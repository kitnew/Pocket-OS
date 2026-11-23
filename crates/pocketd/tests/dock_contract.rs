//! Black-box Dock tests: the real `pocketd` on a private `dbus-daemon`, connected over
//! real TCP + Noise to an in-process desktop agent (`pocket_desktop_agent::server`) with
//! test actions. Each agent runs on its own Tokio runtime so a test can kill it (every
//! socket closes, like a crashed process) and start it again on the same port.
//!
//! Requires `dbus-daemon`. Runs on the host with `cargo test` (scripts/check.sh).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use pocket_desktop_agent::actions::{Action, ActionFuture, Params, Registry};
use pocket_desktop_agent::server::Server;
use pocket_dock_protocol::keys::{Identity, write_private_file};
use pocket_dock_protocol::message::{ActionInfo, ErrorInfo, ParamInfo, ParamKind, Version};
use serde_json::Value;
use tokio::time::timeout;
use zbus::{Connection, OwnedVariant, Variant, proxy};

#[proxy(
    interface = "org.pocketos.Core1",
    default_service = "org.pocketos.Core1",
    default_path = "/org/pocketos/Core1"
)]
trait Core1 {
    fn request_mode(&self, mode: &str) -> zbus::Result<()>;
    fn get_mode(&self) -> zbus::Result<String>;
}

#[proxy(
    interface = "org.pocketos.Core1.Dock",
    default_service = "org.pocketos.Core1",
    default_path = "/org/pocketos/Core1/Dock"
)]
trait Dock {
    fn get_state(&self) -> zbus::Result<HashMap<String, OwnedVariant>>;
    fn list_actions(&self) -> zbus::Result<Vec<HashMap<String, OwnedVariant>>>;
    fn connect(&self) -> zbus::Result<()>;
    fn disconnect(&self) -> zbus::Result<()>;
    fn execute(&self, action: &str, params: HashMap<&str, Variant<'_>>) -> zbus::Result<u64>;
    fn retry(&self, request: u64) -> zbus::Result<u64>;
    fn get_deck(&self) -> zbus::Result<String>;
    fn reload_deck(&self) -> zbus::Result<()>;
    fn set_deck(&self, json: &str) -> zbus::Result<()>;
    fn reset_deck(&self) -> zbus::Result<()>;
    fn get_live(&self) -> zbus::Result<String>;
    fn get_preferences(&self) -> zbus::Result<HashMap<String, OwnedVariant>>;
    fn set_preference(&self, key: &str, value: Variant<'_>) -> zbus::Result<()>;
    #[zbus(signal)]
    fn deck_changed(&self) -> zbus::Result<()>;
    #[zbus(signal)]
    fn live_changed(&self, topic: String, json: String) -> zbus::Result<()>;
    #[zbus(signal)]
    fn notification(&self, json: String) -> zbus::Result<()>;
    #[zbus(signal)]
    fn action_finished(
        &self,
        request: u64,
        action: String,
        ok: bool,
        error_code: String,
        message: String,
        data_json: String,
    ) -> zbus::Result<()>;
    #[zbus(signal)]
    fn actions_changed(&self, actions: Vec<HashMap<String, OwnedVariant>>) -> zbus::Result<()>;
}

const WAIT: Duration = Duration::from_secs(5);

// ── fixtures ─────────────────────────────────────────────────────────────────────────

struct Bus {
    daemon: Child,
    address: String,
}

impl Bus {
    fn start() -> Bus {
        let mut daemon = Command::new("dbus-daemon")
            .args(["--session", "--nofork", "--nopidfile", "--print-address=1"])
            .stdout(Stdio::piped())
            .spawn()
            .expect("dbus-daemon must be installed");
        let mut line = String::new();
        std::io::BufRead::read_line(
            &mut std::io::BufReader::new(daemon.stdout.take().unwrap()),
            &mut line,
        )
        .unwrap();
        Bus {
            daemon,
            address: line.trim().to_owned(),
        }
    }

    /// A core outside any graphical session (no session runtime directory, so no
    /// recovery): it always starts fresh.
    fn core(&self, dock_dir: &Path) -> Child {
        self.core_in(dock_dir, &dock_dir.join("no-session"))
    }

    /// A core whose `XDG_RUNTIME_DIR` is `runtime` (recovery is active when
    /// `runtime/pocket-os` exists, as in a Pocket session).
    fn core_in(&self, dock_dir: &Path, runtime: &Path) -> Child {
        self.command(dock_dir, runtime, &[]).spawn().unwrap()
    }

    /// `pocketd` (or `prefix… pocketd`) with the test environment.
    fn command(&self, dock_dir: &Path, runtime: &Path, prefix: &[&str]) -> Command {
        let bin = std::env::var("POCKETD_BIN").unwrap_or(env!("CARGO_BIN_EXE_pocketd").into());
        let mut c = match prefix.split_first() {
            None => Command::new(bin),
            Some((first, rest)) => {
                let mut c = Command::new(first);
                c.args(rest).arg(bin);
                c
            }
        };
        c.env("DBUS_SESSION_BUS_ADDRESS", &self.address)
            .env("POCKET_DOCK_DIR", dock_dir)
            .env("XDG_RUNTIME_DIR", runtime)
            // No application catalog to scan: these tests are about the Dock, and a
            // host catalog scan (seconds in a debug build) slows parallel starts.
            .env("XDG_DATA_HOME", dock_dir.join("no-data"))
            .env("XDG_DATA_DIRS", dock_dir.join("no-data"))
            .env("POCKETD_LOG", "info,pocketd::dock=debug");
        c
    }

    async fn client(&self) -> Connection {
        zbus::connection::Builder::address(self.address.as_str())
            .build()
            .await
            .unwrap()
    }
}

impl Drop for Bus {
    fn drop(&mut self) {
        let _ = self.daemon.kill();
        let _ = self.daemon.wait();
    }
}

struct Core(Child);
impl Drop for Core {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

impl Core {
    fn terminate(&mut self) -> std::process::ExitStatus {
        let ok = Command::new("kill")
            .args(["-TERM", &self.0.id().to_string()])
            .status()
            .unwrap()
            .success();
        assert!(ok);
        let start = Instant::now();
        loop {
            if let Some(s) = self.0.try_wait().unwrap() {
                return s;
            }
            assert!(start.elapsed() < WAIT, "pocketd did not stop");
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

/// `test.echo` returns its params; `test.slow` takes 10 s; `test.off` is unavailable.
struct TestAction(&'static str);

impl Action for TestAction {
    fn info(&self) -> ActionInfo {
        ActionInfo {
            available: self.0 != "test.off",
            reason: if self.0 == "test.off" {
                "switched off".into()
            } else {
                String::new()
            },
            params: vec![ParamInfo::new("text", ParamKind::String, false)],
            ..ActionInfo::new(self.0, self.0, "test")
        }
    }
    fn execute<'a>(&'a self, params: &'a Params) -> ActionFuture<'a> {
        Box::pin(async move {
            if self.0 == "test.slow" {
                tokio::time::sleep(Duration::from_secs(10)).await;
            }
            if params.get("text").and_then(Value::as_str) == Some("fail") {
                return Err(ErrorInfo::new("failed", "asked to fail"));
            }
            Ok(Value::Object(params.clone()))
        })
    }
}

/// `test.count`: takes 1 s and counts its executions (request-key tests).
struct Counted(Arc<std::sync::atomic::AtomicU32>);

impl Action for Counted {
    fn info(&self) -> ActionInfo {
        ActionInfo::new("test.count", "Count", "test")
    }
    fn execute<'a>(&'a self, _params: &'a Params) -> ActionFuture<'a> {
        Box::pin(async move {
            tokio::time::sleep(Duration::from_secs(1)).await;
            let n = self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
            Ok(serde_json::json!({ "run": n }))
        })
    }
}

struct Agent {
    rt: Option<tokio::runtime::Runtime>,
    live: pocket_desktop_agent::live::Live,
    notes: tokio::sync::broadcast::Sender<pocket_dock_protocol::message::AgentMessage>,
    runs: Arc<std::sync::atomic::AtomicU32>,
}

impl Agent {
    /// Starts an agent trusting `phone` (unless `None`) on 127.0.0.1:`port`.
    fn start(identity: &Identity, phone: Option<&Identity>, port: u16, protocol: Version) -> Agent {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .unwrap();
        let mut reg = Registry::new();
        for id in ["test.echo", "test.slow", "test.off"] {
            reg.register(TestAction(id));
        }
        let runs = Arc::new(std::sync::atomic::AtomicU32::new(0));
        reg.register(Counted(runs.clone()));
        let trusted = phone
            .map(|p| vec![("phone".to_owned(), p.public())])
            .unwrap_or_default();
        let mut server = Server::new(identity.clone(), trusted, "testdesk".into(), Arc::new(reg));
        server.protocol = protocol;
        let (live, notes) = (server.live.clone(), server.notifications.clone());
        // Bind synchronously (the caller is inside another runtime), retrying while the
        // previous agent's port is still being released.
        let listener = (0..100)
            .find_map(|_| {
                std::net::TcpListener::bind(("127.0.0.1", port))
                    .inspect_err(|_| std::thread::sleep(Duration::from_millis(50)))
                    .ok()
            })
            .unwrap_or_else(|| panic!("cannot bind agent port {port}"));
        listener.set_nonblocking(true).unwrap();
        rt.spawn(async move {
            let listener = tokio::net::TcpListener::from_std(listener).unwrap();
            let _ = Arc::new(server).serve(listener).await;
        });
        Agent {
            rt: Some(rt),
            live,
            notes,
            runs,
        }
    }

    /// Kills the agent: every task is dropped and every socket closed.
    fn kill(&mut self) {
        if let Some(rt) = self.rt.take() {
            rt.shutdown_background();
        }
    }
}

impl Drop for Agent {
    fn drop(&mut self) {
        self.kill();
    }
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// A phone configuration directory: identity + host.json pinning `host_key`.
fn phone_dir(name: &str, phone: &Identity, host_key: &Identity, port: u16) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pocket-dock-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    phone.save_new(&dir.join("identity.key")).unwrap();
    let host = serde_json::json!({
        "name": "configured-desk",
        "address": format!("127.0.0.1:{port}"),
        "key": host_key.public().to_string(),
    });
    write_private_file(&dir.join("host.json"), host.to_string().as_bytes(), true).unwrap();
    dir
}

async fn wait_ready(conn: &Connection) {
    timeout(WAIT, async {
        loop {
            let p = zbus::fdo::DBusProxy::new(conn).await.unwrap();
            if p.name_has_owner("org.pocketos.Core1".try_into().unwrap())
                .await
                .unwrap()
            {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("pocketd did not start");
}

fn get<T: TryFrom<OwnedVariant>>(m: &HashMap<String, OwnedVariant>, k: &str) -> T
where
    T::Error: std::fmt::Debug,
{
    T::try_from(m[k].try_clone().unwrap()).unwrap()
}

/// Waits until `state` has the given `state` key (polling GetState; test-only).
async fn wait_state(
    dock: &DockProxy<'_>,
    want: &str,
    within: Duration,
) -> HashMap<String, OwnedVariant> {
    let start = Instant::now();
    loop {
        let s = dock.get_state().await.unwrap();
        if get::<String>(&s, "state") == want {
            return s;
        }
        assert!(
            start.elapsed() < within,
            "dock did not become {want} within {within:?}; state: {:?} {:?}",
            get::<String>(&s, "state"),
            get::<String>(&s, "error")
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

fn error_name(e: zbus::Error) -> String {
    match e {
        zbus::Error::MethodError(name, _, _) => name.as_str().to_owned(),
        other => panic!("expected a D-Bus error, got {other:?}"),
    }
}

fn params(pairs: &[(&'static str, &'static str)]) -> HashMap<&'static str, Variant<'static>> {
    pairs.iter().map(|(k, v)| (*k, Variant::from(*v))).collect()
}

// ── tests ────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn dock_without_a_configured_computer() {
    let bus = Bus::start();
    let dir = std::env::temp_dir().join(format!("pocket-dock-none-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let _core = Core(bus.core(&dir));
    let conn = bus.client().await;
    wait_ready(&conn).await;
    let dock = DockProxy::new(&conn).await.unwrap();
    let core = Core1Proxy::new(&conn).await.unwrap();

    let s = dock.get_state().await.unwrap();
    assert_eq!(get::<String>(&s, "state"), "disconnected");
    assert!(!get::<bool>(&s, "configured"));
    assert!(dock.list_actions().await.unwrap().is_empty());
    assert_eq!(
        error_name(dock.connect().await.unwrap_err()),
        "org.pocketos.Core1.Error.NotConfigured"
    );
    assert_eq!(
        error_name(dock.execute("test.echo", params(&[])).await.unwrap_err()),
        "org.pocketos.Core1.Error.NotConnected"
    );
    // Mode and connection are independent: Dock Mode works without any computer.
    core.request_mode("dock").await.unwrap();
    assert_eq!(core.get_mode().await.unwrap(), "dock");
    assert_eq!(
        get::<String>(&dock.get_state().await.unwrap(), "state"),
        "disconnected"
    );
    core.request_mode("handheld").await.unwrap();
}

#[tokio::test]
async fn connect_discover_execute_and_disconnect() {
    let (phone, desk) = (Identity::generate().unwrap(), Identity::generate().unwrap());
    let port = free_port();
    let _agent = Agent::start(&desk, Some(&phone), port, pocket_dock_protocol::PROTOCOL);
    let dir = phone_dir("ok", &phone, &desk, port);
    let bus = Bus::start();
    let _core = Core(bus.core(&dir));
    let conn = bus.client().await;
    wait_ready(&conn).await;
    let dock = DockProxy::new(&conn).await.unwrap();

    let s = dock.get_state().await.unwrap();
    assert!(get::<bool>(&s, "configured"));
    assert_eq!(get::<String>(&s, "host_name"), "configured-desk");
    assert_eq!(get::<String>(&s, "host_id"), desk.public().short_id());

    let mut finished = dock.receive_action_finished().await.unwrap();
    dock.connect().await.unwrap();
    let s = wait_state(&dock, "connected", WAIT).await;
    assert_eq!(
        get::<String>(&s, "host_name"),
        "testdesk",
        "name reported by the agent"
    );
    assert!(get::<String>(&s, "agent").starts_with("pocket-desktop-agent "));

    // Capability discovery.
    let actions = dock.list_actions().await.unwrap();
    let ids: Vec<String> = actions.iter().map(|a| get(a, "id")).collect();
    assert_eq!(ids, ["test.echo", "test.slow", "test.off", "test.count"]);
    assert!(!get::<bool>(&actions[2], "available"));
    assert_eq!(get::<String>(&actions[2], "reason"), "switched off");
    // Core 1.5: parameters (and their closed choices) reach the phone UI as JSON.
    let declared: Value = serde_json::from_str(&get::<String>(&actions[0], "params")).unwrap();
    assert!(declared.is_array(), "{declared}");

    // Accepted ≠ done: the reply is a request id, the outcome comes from the computer.
    let req = dock
        .execute("test.echo", params(&[("text", "hello")]))
        .await
        .unwrap();
    let sig = timeout(WAIT, finished.next()).await.unwrap().unwrap();
    let a = sig.args().unwrap();
    assert_eq!(
        (a.request, a.action.as_str(), a.ok),
        (req, "test.echo", true)
    );
    assert_eq!(
        serde_json::from_str::<Value>(&a.data_json).unwrap(),
        serde_json::json!({"text": "hello"})
    );

    // Failures reported by the computer.
    let req = dock
        .execute("test.echo", params(&[("text", "fail")]))
        .await
        .unwrap();
    let a = timeout(WAIT, finished.next()).await.unwrap().unwrap();
    let a = a.args().unwrap();
    assert_eq!(
        (a.request, a.ok, a.error_code.as_str()),
        (req, false, "failed")
    );
    let req = dock
        .execute("test.echo", params(&[("bogus", "x")]))
        .await
        .unwrap();
    let a = timeout(WAIT, finished.next()).await.unwrap().unwrap();
    let a = a.args().unwrap();
    assert_eq!(
        (a.request, a.ok, a.error_code.as_str()),
        (req, false, "invalid_params")
    );

    // Rejected before reaching the computer.
    for (action, p, err) in [
        ("no.such.action", params(&[]), "UnknownAction"),
        ("sh -c reboot", params(&[]), "UnknownAction"),
        ("test.off", params(&[]), "ActionUnavailable"),
    ] {
        assert_eq!(
            error_name(dock.execute(action, p).await.unwrap_err()),
            format!("org.pocketos.Core1.Error.{err}")
        );
    }
    let mut bad: HashMap<&str, Variant<'_>> = HashMap::new();
    bad.insert("text", Variant::from(vec![1u8, 2]));
    assert_eq!(
        error_name(dock.execute("test.echo", bad).await.unwrap_err()),
        "org.pocketos.Core1.Error.InvalidArgument"
    );

    // Connect while connected: no-op.
    dock.connect().await.unwrap();
    assert_eq!(
        get::<String>(&dock.get_state().await.unwrap(), "state"),
        "connected"
    );

    dock.disconnect().await.unwrap();
    let s = wait_state(&dock, "disconnected", WAIT).await;
    assert_eq!(get::<u32>(&s, "retry_in"), 0);
    assert!(dock.list_actions().await.unwrap().is_empty());
    assert_eq!(
        error_name(dock.execute("test.echo", params(&[])).await.unwrap_err()),
        "org.pocketos.Core1.Error.NotConnected"
    );
    // Stays disconnected (no background retries after Disconnect).
    tokio::time::sleep(Duration::from_millis(2500)).await;
    assert_eq!(
        get::<String>(&dock.get_state().await.unwrap(), "state"),
        "disconnected"
    );
}

#[tokio::test]
async fn untrusted_phone_is_refused_and_not_retried() {
    let (phone, desk) = (Identity::generate().unwrap(), Identity::generate().unwrap());
    let port = free_port();
    let _agent = Agent::start(&desk, None, port, pocket_dock_protocol::PROTOCOL);
    let dir = phone_dir("untrusted", &phone, &desk, port);
    let bus = Bus::start();
    let _core = Core(bus.core(&dir));
    let conn = bus.client().await;
    wait_ready(&conn).await;
    let dock = DockProxy::new(&conn).await.unwrap();
    dock.connect().await.unwrap();
    let s = wait_state(&dock, "error", WAIT).await;
    assert_eq!(get::<String>(&s, "error_code"), "unauthorized");
    assert!(
        get::<String>(&s, "error").contains(&phone.public().to_string()),
        "tells the user what to trust"
    );
    assert_eq!(get::<u32>(&s, "retry_in"), 0, "needs the user, no retry");
    let rev: u64 = get(&s, "revision");
    tokio::time::sleep(Duration::from_millis(2500)).await;
    assert_eq!(
        get::<u64>(&dock.get_state().await.unwrap(), "revision"),
        rev
    );
}

#[tokio::test]
async fn wrong_host_key_is_refused() {
    let (phone, desk, other) = (
        Identity::generate().unwrap(),
        Identity::generate().unwrap(),
        Identity::generate().unwrap(),
    );
    let port = free_port();
    let _agent = Agent::start(&desk, Some(&phone), port, pocket_dock_protocol::PROTOCOL);
    let dir = phone_dir("mismatch", &phone, &other, port); // pins a different key
    let bus = Bus::start();
    let _core = Core(bus.core(&dir));
    let conn = bus.client().await;
    wait_ready(&conn).await;
    let dock = DockProxy::new(&conn).await.unwrap();
    dock.connect().await.unwrap();
    let s = wait_state(&dock, "error", WAIT).await;
    assert_eq!(get::<String>(&s, "error_code"), "host_key_mismatch");
    assert_eq!(get::<u32>(&s, "retry_in"), 0);
}

#[tokio::test]
async fn incompatible_agent_protocol_is_refused() {
    let (phone, desk) = (Identity::generate().unwrap(), Identity::generate().unwrap());
    let port = free_port();
    let _agent = Agent::start(&desk, Some(&phone), port, Version { major: 2, minor: 0 });
    let dir = phone_dir("proto", &phone, &desk, port);
    let bus = Bus::start();
    let _core = Core(bus.core(&dir));
    let conn = bus.client().await;
    wait_ready(&conn).await;
    let dock = DockProxy::new(&conn).await.unwrap();
    dock.connect().await.unwrap();
    let s = wait_state(&dock, "error", WAIT).await;
    assert_eq!(get::<String>(&s, "error_code"), "incompatible_protocol");
    assert_eq!(get::<u32>(&s, "retry_in"), 0);
}

#[tokio::test]
async fn unavailable_agent_restart_and_disconnection_recover_with_backoff() {
    let (phone, desk) = (Identity::generate().unwrap(), Identity::generate().unwrap());
    let port = free_port();
    let dir = phone_dir("recover", &phone, &desk, port);
    let bus = Bus::start();
    let mut core = Core(bus.core(&dir));
    let conn = bus.client().await;
    wait_ready(&conn).await;
    let dock = DockProxy::new(&conn).await.unwrap();
    let mut finished = dock.receive_action_finished().await.unwrap();

    // 1. Agent not running: an error with a scheduled retry, no fast loop.
    dock.connect().await.unwrap();
    let s = wait_state(&dock, "error", WAIT).await;
    assert_eq!(get::<String>(&s, "error_code"), "unreachable");
    assert_eq!(get::<u32>(&s, "retry_in"), 2);
    let rev: u64 = get(&s, "revision");
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert_eq!(
        get::<u64>(&dock.get_state().await.unwrap(), "revision"),
        rev,
        "no attempt before 2 s"
    );

    // 2. The agent appears: the scheduled retry connects.
    let mut agent = Agent::start(&desk, Some(&phone), port, pocket_dock_protocol::PROTOCOL);
    wait_state(&dock, "connected", Duration::from_secs(8)).await;

    // 3. Agent crash with a request in flight: the request ends as connection_lost,
    //    the link reports the error and schedules a retry.
    let req = dock.execute("test.slow", params(&[])).await.unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    agent.kill();
    let a = timeout(WAIT, finished.next()).await.unwrap().unwrap();
    let a = a.args().unwrap();
    assert_eq!(
        (a.request, a.ok, a.error_code.as_str()),
        (req, false, "connection_lost")
    );
    let s = wait_state(&dock, "error", WAIT).await;
    assert_eq!(get::<String>(&s, "error_code"), "connection_closed");
    assert!(get::<u32>(&s, "retry_in") >= 2);
    assert!(dock.list_actions().await.unwrap().is_empty());

    // 4. Agent restart: reconnects by itself.
    let _agent = Agent::start(&desk, Some(&phone), port, pocket_dock_protocol::PROTOCOL);
    wait_state(&dock, "connected", Duration::from_secs(10)).await;
    assert_eq!(dock.list_actions().await.unwrap().len(), 4);

    // 5. Core restart (and clean shutdown while connected) outside a graphical session:
    //    there is no session recovery, so the new core starts disconnected (connection
    //    state itself is never persisted) and connects on request.
    let status = core.terminate();
    assert!(
        status.success(),
        "pocketd must exit cleanly while connected: {status:?}"
    );
    let _core = Core(bus.core(&dir));
    wait_ready(&conn).await;
    let dock = DockProxy::new(&conn).await.unwrap();
    let s = wait_state(&dock, "disconnected", WAIT).await;
    assert!(get::<bool>(&s, "configured"));
    dock.connect().await.unwrap();
    wait_state(&dock, "connected", WAIT).await;
    let mut finished = dock.receive_action_finished().await.unwrap();
    let req = dock
        .execute("test.echo", params(&[("text", "after restart")]))
        .await
        .unwrap();
    let a = timeout(WAIT, finished.next()).await.unwrap().unwrap();
    assert_eq!(
        (a.args().unwrap().request, a.args().unwrap().ok),
        (req, true)
    );
}

/// A peer that completes the handshake and then never answers (a frozen agent, or a
/// cable pulled without a TCP reset): the action times out and the keepalive ends the
/// session. Takes ~30 s (real ping interval and timeouts).
#[tokio::test]
async fn silent_agent_is_detected_by_keepalive() {
    use pocket_dock_protocol::channel;
    use pocket_dock_protocol::message::{AgentMessage, ClientMessage};

    let (phone, desk) = (Identity::generate().unwrap(), Identity::generate().unwrap());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let desk2 = desk.clone();
    let fake = tokio::spawn(async move {
        let (s, _) = listener.accept().await.unwrap();
        let mut est = channel::respond(s, &desk2).await.unwrap();
        let _hello: Option<ClientMessage> = est.receiver.recv().await.unwrap();
        est.sender
            .send(&AgentMessage::Welcome {
                protocol: pocket_dock_protocol::PROTOCOL,
                agent: "frozen".into(),
                host_name: "frozen".into(),
                actions: vec![TestAction("test.echo").info()],
            })
            .await
            .unwrap();
        // Read and ignore everything (pings, invokes) forever.
        while let Ok(Some(_)) = est.receiver.recv::<ClientMessage>().await {}
        tokio::time::sleep(Duration::from_secs(3600)).await;
    });
    let dir = phone_dir("silent", &phone, &desk, port);
    let bus = Bus::start();
    let _core = Core(bus.core(&dir));
    let conn = bus.client().await;
    wait_ready(&conn).await;
    let dock = DockProxy::new(&conn).await.unwrap();
    let mut finished = dock.receive_action_finished().await.unwrap();
    dock.connect().await.unwrap();
    wait_state(&dock, "connected", WAIT).await;

    let started = Instant::now();
    let req = dock.execute("test.echo", params(&[])).await.unwrap();
    let a = timeout(Duration::from_secs(20), finished.next())
        .await
        .unwrap()
        .unwrap();
    let a = a.args().unwrap();
    assert_eq!(
        (a.request, a.ok, a.error_code.as_str()),
        (req, false, "timeout")
    );
    assert!(started.elapsed() >= Duration::from_secs(14));

    let s = wait_state(&dock, "error", Duration::from_secs(20)).await;
    assert_eq!(get::<String>(&s, "error_code"), "timeout");
    assert!(
        started.elapsed() <= Duration::from_secs(32),
        "detected within ping + pong timeout"
    );
    fake.abort();
}

/// A session runtime directory (`<dir>/pocket-os` exists, as `pocket-session-run` makes it).
fn session_runtime(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pocket-run-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("pocket-os")).unwrap();
    dir
}

/// Issue #5: within a graphical session the mode and the user's link intent survive core
/// restarts (graceful or crash); the link comes back as a new authenticated session;
/// an explicit Disconnect sticks; invalid recovery data means a fresh start.
#[tokio::test]
async fn session_recovery_of_mode_and_link_intent() {
    let (phone, desk) = (Identity::generate().unwrap(), Identity::generate().unwrap());
    let port = free_port();
    let _agent = Agent::start(&desk, Some(&phone), port, pocket_dock_protocol::PROTOCOL);
    let dir = phone_dir("recovery", &phone, &desk, port);
    let run = session_runtime("recovery");
    let file = run.join("pocket-os/core-recovery.json");
    let bus = Bus::start();
    let conn = bus.client().await;

    // New session: handheld, nothing wanted, nothing written yet.
    let mut core = Core(bus.core_in(&dir, &run));
    wait_ready(&conn).await;
    let core1 = Core1Proxy::new(&conn).await.unwrap();
    let dock = DockProxy::new(&conn).await.unwrap();
    assert_eq!(core1.get_mode().await.unwrap(), "handheld");
    let s = dock.get_state().await.unwrap();
    assert!(!get::<bool>(&s, "connection_wanted"));
    assert!(!file.exists());

    // 1. Handheld → restart → handheld.
    core.terminate();
    let mut core = Core(bus.core_in(&dir, &run));
    wait_ready(&conn).await;
    assert_eq!(core1.get_mode().await.unwrap(), "handheld");

    // Entering Dock Mode connects (Core policy, ADR-0007) and records the intent.
    core1.request_mode("dock").await.unwrap();
    let s = wait_state(&dock, "connected", WAIT).await;
    assert!(get::<bool>(&s, "connection_wanted"));
    let saved: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(
        saved,
        serde_json::json!({"version": 1, "mode": "dock", "dock_wanted": true})
    );
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&file).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    // 2+3. Dock + connected → crash (SIGKILL) → dock, and a new authenticated session.
    let _ = core.0.kill();
    let _ = core.0.wait();
    let mut core = Core(bus.core_in(&dir, &run));
    wait_ready(&conn).await;
    assert_eq!(core1.get_mode().await.unwrap(), "dock");
    let s = wait_state(&dock, "connected", WAIT).await;
    assert!(get::<bool>(&s, "connection_wanted"));
    assert_eq!(
        dock.list_actions().await.unwrap().len(),
        4,
        "actions resynchronised"
    );
    let mut finished = dock.receive_action_finished().await.unwrap();
    let req = dock
        .execute("test.echo", params(&[("text", "restored")]))
        .await
        .unwrap();
    let a = timeout(WAIT, finished.next()).await.unwrap().unwrap();
    assert_eq!(
        (a.args().unwrap().request, a.args().unwrap().ok),
        (req, true)
    );
    // An unread signal stream eventually stalls this connection (zbus applies
    // backpressure), so it is not kept around.
    drop(finished);

    // 11. Repeated restarts: each core ends in exactly one session, no stale state.
    for _ in 0..3 {
        core.terminate();
        core = Core(bus.core_in(&dir, &run));
        wait_ready(&conn).await;
        assert_eq!(core1.get_mode().await.unwrap(), "dock");
        wait_state(&dock, "connected", WAIT).await;
    }
    assert_eq!(
        std::fs::read_dir(run.join("pocket-os")).unwrap().count(),
        1,
        "only the recovery file, no temporary files"
    );

    // 5. Manual Disconnect → restart → still dock, and it stays disconnected.
    dock.disconnect().await.unwrap();
    let s = wait_state(&dock, "disconnected", WAIT).await;
    assert!(!get::<bool>(&s, "connection_wanted"));
    core.terminate();
    let mut core = Core(bus.core_in(&dir, &run));
    wait_ready(&conn).await;
    assert_eq!(core1.get_mode().await.unwrap(), "dock");
    tokio::time::sleep(Duration::from_millis(2500)).await;
    let s = dock.get_state().await.unwrap();
    assert_eq!(get::<String>(&s, "state"), "disconnected");
    assert!(!get::<bool>(&s, "connection_wanted"));
    // Requesting the mode it is already in is not "entering" Dock Mode: still disconnected.
    core1.request_mode("dock").await.unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        get::<String>(&dock.get_state().await.unwrap(), "state"),
        "disconnected"
    );

    // 7. An explicit Connect after the Disconnect works and is recorded again.
    dock.connect().await.unwrap();
    let s = wait_state(&dock, "connected", WAIT).await;
    assert!(get::<bool>(&s, "connection_wanted"));

    // 8. Dock → handheld → restart → handheld (the wanted link stays up in handheld).
    core1.request_mode("handheld").await.unwrap();
    core.terminate();
    let mut core = Core(bus.core_in(&dir, &run));
    wait_ready(&conn).await;
    assert_eq!(core1.get_mode().await.unwrap(), "handheld");
    wait_state(&dock, "connected", WAIT).await;

    // 4. Dock + computer unavailable → restart → still dock, retrying, not "connected".
    core1.request_mode("dock").await.unwrap();
    drop(_agent);
    wait_state(&dock, "error", WAIT).await;
    core.terminate();
    let mut core = Core(bus.core_in(&dir, &run));
    wait_ready(&conn).await;
    assert_eq!(core1.get_mode().await.unwrap(), "dock");
    let s = wait_state(&dock, "error", WAIT).await;
    assert_eq!(get::<String>(&s, "error_code"), "unreachable");
    assert!(get::<u32>(&s, "retry_in_ms") > 0);
    assert!(get::<bool>(&s, "connection_wanted"));

    // 10. Invalid recovery data → a fresh start (handheld, nothing wanted), no crash.
    core.terminate();
    std::fs::write(&file, b"{\"version\":1,\"mode\":\"do").unwrap();
    let _core = Core(bus.core_in(&dir, &run));
    wait_ready(&conn).await;
    assert_eq!(core1.get_mode().await.unwrap(), "handheld");
    let s = dock.get_state().await.unwrap();
    assert_eq!(get::<String>(&s, "state"), "disconnected");
    assert!(!get::<bool>(&s, "connection_wanted"));
    let _ = std::fs::remove_dir_all(&run);
}

/// The retry countdown is computed when the state is read, so it decreases between
/// reads and a late reader (a restarted shell) sees the real time left.
#[tokio::test]
async fn retry_countdown_follows_the_deadline() {
    let (phone, desk) = (Identity::generate().unwrap(), Identity::generate().unwrap());
    let port = free_port();
    let dir = phone_dir("countdown", &phone, &desk, port);
    let bus = Bus::start();
    let _core = Core(bus.core(&dir));
    let conn = bus.client().await;
    wait_ready(&conn).await;
    let dock = DockProxy::new(&conn).await.unwrap();
    dock.connect().await.unwrap();
    // Wait for the 4 s wait (second failure) so there is room to observe it.
    let start = Instant::now();
    let (first, rev) = loop {
        let s = dock.get_state().await.unwrap();
        if get::<String>(&s, "state") == "error" && get::<u32>(&s, "retry_in_ms") > 2500 {
            break (get::<u32>(&s, "retry_in_ms"), get::<u64>(&s, "revision"));
        }
        assert!(start.elapsed() < Duration::from_secs(8));
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    assert!(first <= 4000);
    tokio::time::sleep(Duration::from_millis(1000)).await;
    let s = dock.get_state().await.unwrap();
    assert_eq!(get::<u64>(&s, "revision"), rev, "same scheduled retry");
    let later: u32 = get(&s, "retry_in_ms");
    assert!(
        (first - later) >= 900 && (first - later) <= 1300,
        "{first} → {later}"
    );
    assert_eq!(get::<u32>(&s, "retry_in"), later.div_ceil(1000));
    // Retry now: no retry scheduled while connecting; Disconnect clears it.
    dock.connect().await.unwrap();
    dock.disconnect().await.unwrap();
    let s = wait_state(&dock, "disconnected", WAIT).await;
    assert_eq!(get::<u32>(&s, "retry_in_ms"), 0);
    assert_eq!(get::<u32>(&s, "retry_in"), 0);
}

/// Issue #7 through the real kernel path: the core runs in its own network namespace
/// (`unshare -rn`, no root needed) where adding an interface produces genuine rtnetlink
/// notifications. A long pending retry is pulled forward once, a burst of further changes
/// does not cause more attempts, and nothing happens after Disconnect.
#[tokio::test]
async fn network_change_pulls_a_pending_retry_forward() {
    use std::process::Stdio;
    let ids = |flag: &str| {
        String::from_utf8(Command::new("id").arg(flag).output().unwrap().stdout)
            .unwrap()
            .trim()
            .to_owned()
    };
    let (uid, gid) = (ids("-u"), ids("-g"));
    // Everything the test needs: an unprivileged netns, dummy links, nested uid mapping.
    let probe = format!(
        "ip link set lo up && ip link add p0 type dummy && unshare -U --map-user={uid} --map-group={gid} true"
    );
    if !Command::new("unshare")
        .args(["-rn", "sh", "-c", &probe])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
    {
        eprintln!(
            "SKIP: unprivileged network namespaces (util-linux unshare, dummy links) unavailable"
        );
        return;
    }
    let (phone, desk) = (Identity::generate().unwrap(), Identity::generate().unwrap());
    let port = free_port();
    let dir = phone_dir("netns", &phone, &desk, port);
    let ctl = dir.join("ctl");
    let bus = Bus::start();
    // In the namespace: loopback up (the configured address, nothing listens there),
    // then run pocketd while a helper adds dummy interfaces when the test asks for it.
    // pocketd runs in a nested user namespace mapped back to our uid: the bus
    // authenticates it by uid (EXTERNAL), and namespace-root would not match.
    let script = format!(
        r#"ip link set lo up || exit 1
unshare -U --map-user={uid} --map-group={gid} "$0" & core=$!
n=0
while kill -0 $core 2>/dev/null; do
  if [ -e "{ctl}" ]; then
    rm -f "{ctl}"; n=$((n+1)); ip link add d$n type dummy; ip link set d$n up
  fi
  sleep 0.05
done"#,
        ctl = ctl.display(),
    );
    let _core = Core(
        bus.command(
            &dir,
            &dir.join("no-session"),
            &["unshare", "-rn", "sh", "-c", &script],
        )
        .spawn()
        .unwrap(),
    );
    let conn = bus.client().await;
    wait_ready(&conn).await;
    let dock = DockProxy::new(&conn).await.unwrap();
    let mut states = dock.inner().receive_signal("StateChanged").await.unwrap();
    let trigger = || std::fs::write(&ctl, b"").unwrap();

    // Fail until the wait is 8 s (attempts at 0, 2, 6 s).
    dock.connect().await.unwrap();
    let start = Instant::now();
    loop {
        let s = dock.get_state().await.unwrap();
        if get::<String>(&s, "state") == "error" && get::<u32>(&s, "retry_in_ms") > 6000 {
            break;
        }
        assert!(
            start.elapsed() < Duration::from_secs(12),
            "backoff did not reach 8 s"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    // Past MIN_GAP after that attempt, so the hint's own settle delay is what we see.
    tokio::time::sleep(Duration::from_millis(1500)).await;
    while timeout(Duration::from_millis(100), states.next())
        .await
        .is_ok()
    {}

    // A carrier-up edge: the next attempt starts ~1 s later instead of ~7 s later.
    let hinted = Instant::now();
    trigger();
    let mut connecting = Vec::new();
    let deadline = Instant::now() + Duration::from_millis(2500); // before the 2 s backoff retry
    while Instant::now() < deadline {
        if let Ok(Some(m)) = timeout(Duration::from_millis(100), states.next()).await {
            let (d,): (HashMap<String, OwnedVariant>,) = m.body().deserialize().unwrap();
            if get::<String>(&d, "state") == "connecting" {
                connecting.push(hinted.elapsed());
            }
        }
    }
    assert_eq!(connecting.len(), 1, "one early attempt: {connecting:?}");
    assert!(
        connecting[0] >= Duration::from_millis(800) && connecting[0] <= Duration::from_millis(1700),
        "attempt {:?} after the hint",
        connecting[0]
    );
    // The backoff restarted: the next wait is 2 s again.
    let s = wait_state(&dock, "error", WAIT).await;
    assert!(get::<u32>(&s, "retry_in_ms") <= 2000);

    // A burst of changes inside the cooldown: no attempts beyond the normal backoff
    // (2 s, then 4 s): at most two in the next 5 s.
    for _ in 0..5 {
        trigger();
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
    let mut attempts = 0;
    let deadline = Instant::now() + Duration::from_millis(4200);
    while Instant::now() < deadline {
        if let Ok(Some(m)) = timeout(Duration::from_millis(100), states.next()).await {
            let (d,): (HashMap<String, OwnedVariant>,) = m.body().deserialize().unwrap();
            if get::<String>(&d, "state") == "connecting" {
                attempts += 1;
            }
        }
    }
    assert!(
        attempts <= 2,
        "{attempts} attempts during a burst of network changes"
    );

    // After Disconnect, network changes do nothing.
    dock.disconnect().await.unwrap();
    wait_state(&dock, "disconnected", WAIT).await;
    tokio::time::sleep(Duration::from_secs(11)).await; // past the cooldown
    let rev: u64 = get(&dock.get_state().await.unwrap(), "revision");
    trigger();
    tokio::time::sleep(Duration::from_millis(2500)).await;
    let s = dock.get_state().await.unwrap();
    assert_eq!(get::<String>(&s, "state"), "disconnected");
    assert_eq!(get::<u64>(&s, "revision"), rev);
}

// ── Stage 4 (Core 1.4) ───────────────────────────────────────────────────────────────

/// A connected phone (pocketd on a private bus) and its agent.
async fn connected(name: &str, protocol: Version) -> (Agent, Bus, Core, Connection, PathBuf) {
    let (phone, desk) = (Identity::generate().unwrap(), Identity::generate().unwrap());
    let port = free_port();
    let agent = Agent::start(&desk, Some(&phone), port, protocol);
    let dir = phone_dir(name, &phone, &desk, port);
    let bus = Bus::start();
    let core = Core(bus.core(&dir));
    let conn = bus.client().await;
    wait_ready(&conn).await;
    let dock = DockProxy::new(&conn).await.unwrap();
    dock.connect().await.unwrap();
    wait_state(&dock, "connected", WAIT).await;
    (agent, bus, core, conn, dir)
}

#[tokio::test]
async fn deck_is_validated_reloadable_and_survives_a_core_restart() {
    let bus = Bus::start();
    let dir = std::env::temp_dir().join(format!("pocket-dock-deck-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut core = Core(bus.core(&dir));
    let conn = bus.client().await;
    wait_ready(&conn).await;
    let dock = DockProxy::new(&conn).await.unwrap();
    let deck: Value = serde_json::from_str(&dock.get_deck().await.unwrap()).unwrap();
    assert_eq!(deck["source"], "default");
    assert_eq!(deck["navigation"]["idle_timeout_s"], 15);
    assert_eq!(deck["pages"][0]["id"], "desk");

    let user = serde_json::json!({"version": 1, "navigation": {"idle_timeout_s": 30},
        "pages": [{"id": "mine", "title": "Mine", "tiles": [{"action": "test.echo", "label": "Echo"}]}]});
    std::fs::write(dir.join("deck.json"), user.to_string()).unwrap();
    let mut changed = dock.receive_deck_changed().await.unwrap();
    dock.reload_deck().await.unwrap();
    timeout(WAIT, changed.next()).await.unwrap().unwrap();
    let deck: Value = serde_json::from_str(&dock.get_deck().await.unwrap()).unwrap();
    assert_eq!(
        (deck["source"].as_str(), deck["pages"][0]["id"].as_str()),
        (Some("user"), Some("mine"))
    );
    assert_eq!(deck["navigation"]["idle_timeout_s"], 30);

    // A core restart reads the same configuration.
    core.terminate();
    let _core = Core(bus.core(&dir));
    wait_ready(&conn).await;
    let dock = DockProxy::new(&conn).await.unwrap();
    let deck: Value = serde_json::from_str(&dock.get_deck().await.unwrap()).unwrap();
    assert_eq!(deck["pages"][0]["id"], "mine");

    // An invalid user file: the default deck, with the reason.
    std::fs::write(dir.join("deck.json"), r#"{"version":1,"pages":[{"id":"x","title":"x","tiles":[{"action":"rm -rf /","label":"x"}]}]}"#).unwrap();
    dock.reload_deck().await.unwrap();
    let deck: Value = serde_json::from_str(&dock.get_deck().await.unwrap()).unwrap();
    assert_eq!(deck["source"], "default");
    assert!(
        deck["error"].as_str().unwrap().contains("action"),
        "{}",
        deck["error"]
    );
}

#[tokio::test]
async fn deck_edits_are_validated_persisted_and_resettable() {
    let bus = Bus::start();
    let dir = std::env::temp_dir().join(format!("pocket-dock-deck-edit-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut core = Core(bus.core(&dir));
    let conn = bus.client().await;
    wait_ready(&conn).await;
    let dock = DockProxy::new(&conn).await.unwrap();
    let mut changed = dock.receive_deck_changed().await.unwrap();

    // Edit what GetDeck returned (rename a page, drop a tile) and send it back.
    let mut deck: Value = serde_json::from_str(&dock.get_deck().await.unwrap()).unwrap();
    deck["pages"][0]["title"] = Value::from("Desk");
    deck["pages"][0]["tiles"].as_array_mut().unwrap().remove(0);
    dock.set_deck(&deck.to_string()).await.unwrap();
    timeout(WAIT, changed.next()).await.unwrap().unwrap();
    let now: Value = serde_json::from_str(&dock.get_deck().await.unwrap()).unwrap();
    assert_eq!(
        (now["source"].as_str(), now["pages"][0]["title"].as_str()),
        (Some("user"), Some("Desk"))
    );

    // Invalid edits are refused and change nothing.
    let mut bad = now.clone();
    bad["pages"][0]["tiles"][0]["action"] = Value::from("sh -c x");
    let e = dock.set_deck(&bad.to_string()).await.unwrap_err();
    assert!(e.to_string().contains("InvalidArgument"), "{e}");
    assert_eq!(
        serde_json::from_str::<Value>(&dock.get_deck().await.unwrap()).unwrap(),
        now
    );

    // Survives a Core restart.
    core.terminate();
    let _core = Core(bus.core(&dir));
    wait_ready(&conn).await;
    let dock = DockProxy::new(&conn).await.unwrap();
    let after: Value = serde_json::from_str(&dock.get_deck().await.unwrap()).unwrap();
    assert_eq!(after["pages"][0]["title"], "Desk");

    dock.reset_deck().await.unwrap();
    let reset: Value = serde_json::from_str(&dock.get_deck().await.unwrap()).unwrap();
    assert_eq!(
        (
            reset["source"].as_str(),
            reset["pages"][0]["title"].as_str()
        ),
        (Some("default"), Some("Desk"))
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

#[tokio::test]
async fn an_uncertain_request_is_retried_with_its_key_and_never_runs_twice() {
    let (agent, _bus, _core, conn, _dir) = connected("retry", pocket_dock_protocol::PROTOCOL).await;
    let dock = DockProxy::new(&conn).await.unwrap();
    let mut finished = dock.receive_action_finished().await.unwrap();

    let req = dock.execute("test.count", params(&[])).await.unwrap();
    // The link goes away while the computer is still running it (1 s).
    tokio::time::sleep(Duration::from_millis(200)).await;
    dock.disconnect().await.unwrap();
    let a = timeout(WAIT, finished.next()).await.unwrap().unwrap();
    let a = a.args().unwrap();
    assert_eq!(
        (a.request, a.ok, a.error_code.as_str()),
        (req, false, "connection_lost")
    );

    // Nothing is re-sent automatically, not even after reconnecting.
    dock.connect().await.unwrap();
    wait_state(&dock, "connected", WAIT).await;
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert_eq!(
        agent.runs.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "it did run once"
    );

    // The user retries: the computer returns the stored result instead of running again.
    let again = dock.retry(req).await.unwrap();
    assert_ne!(again, req);
    let a = timeout(WAIT, finished.next()).await.unwrap().unwrap();
    let a = a.args().unwrap();
    assert_eq!(
        (a.request, a.ok, a.message.as_str()),
        (again, true, "duplicate")
    );
    assert_eq!(
        serde_json::from_str::<Value>(&a.data_json).unwrap(),
        serde_json::json!({"run": 1})
    );
    assert_eq!(
        agent.runs.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "executed exactly once"
    );

    // A request can be retried once; a known outcome cannot be retried at all.
    assert_eq!(
        error_name(dock.retry(req).await.unwrap_err()),
        "org.pocketos.Core1.Error.UnknownRequest"
    );
    assert_eq!(
        error_name(dock.retry(again).await.unwrap_err()),
        "org.pocketos.Core1.Error.UnknownRequest"
    );
    assert_eq!(
        error_name(dock.retry(999).await.unwrap_err()),
        "org.pocketos.Core1.Error.UnknownRequest"
    );
}

#[tokio::test]
async fn live_state_is_relayed_and_cleared_when_the_session_ends() {
    let (agent, _bus, _core, conn, _dir) = connected("live", pocket_dock_protocol::PROTOCOL).await;
    let dock = DockProxy::new(&conn).await.unwrap();
    let mut live = dock.receive_live_changed().await.unwrap();
    agent.live.set(
        "media",
        true,
        "",
        0,
        serde_json::json!({"active": "elisa", "players": []}),
    );
    let sig = timeout(WAIT, async {
        loop {
            let s = live.next().await.unwrap();
            if s.args().unwrap().topic == "media" {
                return s;
            }
        }
    })
    .await
    .unwrap();
    let v: Value = serde_json::from_str(&sig.args().unwrap().json).unwrap();
    assert_eq!(v["data"]["active"], "elisa");
    let all: Value = serde_json::from_str(&dock.get_live().await.unwrap()).unwrap();
    assert_eq!(all["media"]["data"]["active"], "elisa");
    assert_eq!(
        all["host"]["available"], false,
        "topics the agent does not provide are unavailable"
    );

    dock.disconnect().await.unwrap();
    wait_state(&dock, "disconnected", WAIT).await;
    assert_eq!(
        dock.get_live().await.unwrap(),
        "{}",
        "no stale remote state"
    );

    // Reconnect: a fresh snapshot.
    dock.connect().await.unwrap();
    wait_state(&dock, "connected", WAIT).await;
    timeout(WAIT, async {
        loop {
            let all: Value = serde_json::from_str(&dock.get_live().await.unwrap()).unwrap();
            if all["media"]["data"]["active"] == "elisa" {
                return;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn a_1_0_agent_still_works_without_live_state() {
    let old = Version { major: 1, minor: 0 };
    let (_agent, _bus, _core, conn, _dir) = connected("old", old).await;
    let dock = DockProxy::new(&conn).await.unwrap();
    let mut finished = dock.receive_action_finished().await.unwrap();
    let req = dock
        .execute("test.echo", params(&[("text", "x")]))
        .await
        .unwrap();
    let a = timeout(WAIT, finished.next()).await.unwrap().unwrap();
    assert_eq!(
        (a.args().unwrap().request, a.args().unwrap().ok),
        (req, true)
    );
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(dock.get_live().await.unwrap(), "{}");
}

#[tokio::test]
async fn notifications_follow_the_phone_preferences_which_persist() {
    let (agent, bus, mut core, conn, dir) =
        connected("notes", pocket_dock_protocol::PROTOCOL).await;
    let dock = DockProxy::new(&conn).await.unwrap();
    let prefs = dock.get_preferences().await.unwrap();
    assert!(!get::<bool>(&prefs, "dnd"));
    let note = |app: &str| {
        pocket_dock_protocol::message::AgentMessage::Notification(
            pocket_dock_protocol::message::Notification {
                nid: format!("n-{app}"),
                app: app.into(),
                summary: "Hello".into(),
                body: "Body".into(),
                urgency: 1,
                redacted: false,
                time_ms: 1,
            },
        )
    };
    // Wait until the link has subscribed (the snapshot of `notifications` arrived).
    timeout(WAIT, async {
        while !dock.get_live().await.unwrap().contains("notifications") {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    let mut notes = dock.receive_notification().await.unwrap();
    agent.notes.send(note("Mail")).unwrap();
    let n: Value = serde_json::from_str(
        &timeout(WAIT, notes.next())
            .await
            .unwrap()
            .unwrap()
            .args()
            .unwrap()
            .json,
    )
    .unwrap();
    assert_eq!(
        (n["app"].as_str(), n["silent"].as_bool()),
        (Some("Mail"), Some(false))
    );

    dock.set_preference("mute_app", Variant::from("mail"))
        .await
        .unwrap();
    dock.set_preference("dnd", Variant::from(true))
        .await
        .unwrap();
    assert_eq!(
        error_name(
            dock.set_preference("dnd", Variant::from("yes"))
                .await
                .unwrap_err()
        ),
        "org.pocketos.Core1.Error.InvalidArgument"
    );
    agent.notes.send(note("Mail")).unwrap();
    agent.notes.send(note("Chat")).unwrap();
    let n: Value = serde_json::from_str(
        &timeout(WAIT, notes.next())
            .await
            .unwrap()
            .unwrap()
            .args()
            .unwrap()
            .json,
    )
    .unwrap();
    assert_eq!(
        (n["app"].as_str(), n["silent"].as_bool()),
        (Some("Chat"), Some(true)),
        "muted app dropped; DND = silent"
    );

    // Preferences survive a core restart.
    core.terminate();
    let _core = Core(bus.core(&dir));
    wait_ready(&conn).await;
    let dock = DockProxy::new(&conn).await.unwrap();
    let prefs = dock.get_preferences().await.unwrap();
    assert!(get::<bool>(&prefs, "dnd"));
    assert_eq!(get::<Vec<String>>(&prefs, "muted_apps"), ["mail"]);
}
