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

    fn core(&self, dock_dir: &Path) -> Child {
        let bin = std::env::var("POCKETD_BIN").unwrap_or(env!("CARGO_BIN_EXE_pocketd").into());
        Command::new(bin)
            .env("DBUS_SESSION_BUS_ADDRESS", &self.address)
            .env("POCKET_DOCK_DIR", dock_dir)
            .env("POCKETD_LOG", "info")
            .spawn()
            .unwrap()
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
            id: self.0.into(),
            label: self.0.into(),
            category: "test".into(),
            available: self.0 != "test.off",
            reason: if self.0 == "test.off" {
                "switched off".into()
            } else {
                String::new()
            },
            params: vec![ParamInfo {
                name: "text".into(),
                kind: ParamKind::String,
                required: false,
            }],
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

struct Agent {
    rt: Option<tokio::runtime::Runtime>,
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
        let trusted = phone
            .map(|p| vec![("phone".to_owned(), p.public())])
            .unwrap_or_default();
        let mut server = Server::new(identity.clone(), trusted, "testdesk".into(), Arc::new(reg));
        server.protocol = protocol;
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
        Agent { rt: Some(rt) }
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
    assert_eq!(ids, ["test.echo", "test.slow", "test.off"]);
    assert!(!get::<bool>(&actions[2], "available"));
    assert_eq!(get::<String>(&actions[2], "reason"), "switched off");

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
    assert_eq!(dock.list_actions().await.unwrap().len(), 3);

    // 5. Core restart (and clean shutdown while connected): the new core starts
    //    disconnected (connection state is not persisted) and connects on request.
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
