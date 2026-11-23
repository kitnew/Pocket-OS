//! Protocol tests: a real agent `Server` on 127.0.0.1, real TCP + Noise, a raw client.
//! The desktop-notification test also runs a private `dbus-daemon` with a fake
//! notification server, so nothing reaches the real desktop.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use pocket_desktop_agent::actions::{Action, ActionFuture, Changes, Desktop, Params, Registry};
use pocket_desktop_agent::mpris::Mpris;
use pocket_desktop_agent::server::Server;
use pocket_dock_protocol::channel::{self, Established};
use pocket_dock_protocol::keys::Identity;
use pocket_dock_protocol::message::{ActionInfo, AgentMessage, ClientMessage, Version};
use pocket_dock_protocol::{PROTOCOL, error_code};
use serde_json::{Value, json};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::timeout;

const WAIT: Duration = Duration::from_secs(5);

/// `test.echo` returns its params; `test.fail` fails; `test.toggle` follows a flag.
struct TestAction {
    id: &'static str,
    available: Arc<AtomicBool>,
}

impl Action for TestAction {
    fn info(&self) -> ActionInfo {
        let available = self.available.load(Ordering::SeqCst);
        ActionInfo {
            available,
            reason: if available {
                String::new()
            } else {
                "switched off".into()
            },
            params: vec![pocket_dock_protocol::message::ParamInfo::new(
                "text",
                pocket_dock_protocol::message::ParamKind::String,
                false,
            )],
            ..ActionInfo::new(self.id, self.id, "test")
        }
    }
    fn execute<'a>(&'a self, params: &'a Params) -> ActionFuture<'a> {
        Box::pin(async move {
            if self.id == "test.fail" {
                return Err(pocket_dock_protocol::message::ErrorInfo::new(
                    error_code::FAILED,
                    "it broke",
                ));
            }
            Ok(Value::Object(params.clone()))
        })
    }
}

struct Agent {
    addr: std::net::SocketAddr,
    key: pocket_dock_protocol::keys::PublicKey,
    toggle: Arc<AtomicBool>,
    changes: Changes,
    live: pocket_desktop_agent::live::Live,
    notes: tokio::sync::broadcast::Sender<AgentMessage>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for Agent {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn agent(phone: &Identity, configure: impl FnOnce(&mut Registry)) -> Agent {
    agent_with(phone, configure, |_| {}).await
}

async fn agent_with(
    phone: &Identity,
    configure: impl FnOnce(&mut Registry),
    tune: impl FnOnce(&mut Server),
) -> Agent {
    let id = Identity::generate().unwrap();
    let key = id.public();
    let toggle = Arc::new(AtomicBool::new(true));
    let mut reg = Registry::new();
    for name in ["test.echo", "test.fail"] {
        reg.register(TestAction {
            id: name,
            available: Arc::new(AtomicBool::new(true)),
        });
    }
    reg.register(TestAction {
        id: "test.toggle",
        available: toggle.clone(),
    });
    let changes = reg.changes();
    configure(&mut reg);
    let mut server = Server::new(
        id,
        vec![("phone".into(), phone.public())],
        "testhost".into(),
        Arc::new(reg),
    );
    tune(&mut server);
    let (live, notes) = (server.live.clone(), server.notifications.clone());
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        let _ = Arc::new(server).serve(listener).await;
    });
    Agent {
        addr,
        key,
        toggle,
        changes,
        live,
        notes,
        task,
    }
}

type Conn = Established<TcpStream>;

async fn connect(a: &Agent, phone: &Identity) -> Conn {
    let s = TcpStream::connect(a.addr).await.unwrap();
    let c = timeout(WAIT, channel::initiate(s, phone))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(c.remote, a.key, "agent key must be the one we pin");
    c
}

async fn recv(c: &mut Conn) -> Option<AgentMessage> {
    timeout(WAIT, c.receiver.recv::<AgentMessage>())
        .await
        .expect("agent reply timed out")
        .unwrap()
}

async fn hello(c: &mut Conn, protocol: Version) -> AgentMessage {
    c.sender
        .send(&ClientMessage::Hello {
            protocol,
            client: "test".into(),
            device_name: "test-phone".into(),
        })
        .await
        .unwrap();
    recv(c).await.unwrap()
}

async fn session(a: &Agent, phone: &Identity) -> (Conn, Vec<ActionInfo>) {
    let mut c = connect(a, phone).await;
    match hello(&mut c, PROTOCOL).await {
        AgentMessage::Welcome {
            protocol,
            host_name,
            actions,
            ..
        } => {
            assert_eq!(protocol, PROTOCOL);
            assert_eq!(host_name, "testhost");
            (c, actions)
        }
        other => panic!("expected welcome, got {other:?}"),
    }
}

async fn invoke(c: &mut Conn, id: u64, action: &str, params: Value) -> AgentMessage {
    invoke_key(c, id, action, params, None).await
}

async fn invoke_key(
    c: &mut Conn,
    id: u64,
    action: &str,
    params: Value,
    key: Option<&str>,
) -> AgentMessage {
    c.sender
        .send(&ClientMessage::Invoke {
            id,
            action: action.into(),
            params: params.as_object().unwrap().clone(),
            key: key.map(str::to_owned),
        })
        .await
        .unwrap();
    loop {
        match recv(c).await.unwrap() {
            AgentMessage::Actions { .. } | AgentMessage::Progress { .. } => continue,
            m => return m,
        }
    }
}

fn error_code_of(m: &AgentMessage) -> &str {
    match m {
        AgentMessage::Result {
            ok: false,
            error: Some(e),
            ..
        } => &e.code,
        AgentMessage::Error { code, .. } => code,
        other => panic!("expected an error, got {other:?}"),
    }
}

#[tokio::test]
async fn trusted_phone_gets_welcome_and_capabilities() {
    let phone = Identity::generate().unwrap();
    let a = agent(&phone, |_| {}).await;
    let (_c, actions) = session(&a, &phone).await;
    let ids: Vec<_> = actions.iter().map(|a| a.id.as_str()).collect();
    assert_eq!(ids, ["test.echo", "test.fail", "test.toggle"]);
    assert!(actions.iter().all(|a| a.available));
}

#[tokio::test]
async fn untrusted_phone_is_rejected_before_any_action() {
    let phone = Identity::generate().unwrap();
    let stranger = Identity::generate().unwrap();
    let a = agent(&phone, |_| {}).await;
    let mut c = connect(&a, &stranger).await;
    let m = recv(&mut c).await.unwrap();
    assert_eq!(error_code_of(&m), error_code::UNAUTHORIZED);
    // Whatever the stranger sends next, nothing executes: the agent closed the session.
    let _ = c
        .sender
        .send(&ClientMessage::Invoke {
            id: 1,
            action: "test.echo".into(),
            params: Default::default(),
            key: None,
        })
        .await;
    assert!(recv(&mut c).await.is_none(), "connection must be closed");
}

#[tokio::test]
async fn incompatible_protocol_is_refused() {
    let phone = Identity::generate().unwrap();
    let a = agent(&phone, |_| {}).await;
    let mut c = connect(&a, &phone).await;
    let m = hello(&mut c, Version { major: 2, minor: 0 }).await;
    assert_eq!(error_code_of(&m), error_code::INCOMPATIBLE_PROTOCOL);
    assert!(recv(&mut c).await.is_none());

    // And a newer minor of the same major is accepted.
    let mut c = connect(&a, &phone).await;
    let m = hello(
        &mut c,
        Version {
            major: PROTOCOL.major,
            minor: PROTOCOL.minor + 3,
        },
    )
    .await;
    assert!(matches!(m, AgentMessage::Welcome { .. }));
}

#[tokio::test]
async fn invoke_before_hello_is_a_protocol_error() {
    let phone = Identity::generate().unwrap();
    let a = agent(&phone, |_| {}).await;
    let mut c = connect(&a, &phone).await;
    let m = invoke(&mut c, 1, "test.echo", json!({})).await;
    assert_eq!(error_code_of(&m), error_code::PROTOCOL_ERROR);
}

#[tokio::test]
async fn actions_execute_and_errors_are_structured() {
    let phone = Identity::generate().unwrap();
    let a = agent(&phone, |r| {
        r.set_enabled(Some(["test.echo".to_string(), "test.toggle".to_string()]))
    })
    .await;
    let (mut c, actions) = session(&a, &phone).await;
    assert_eq!(actions.len(), 2, "disabled actions are not advertised");

    let m = invoke(&mut c, 1, "test.echo", json!({"text": "hi"})).await;
    assert_eq!(
        m,
        AgentMessage::Result {
            id: 1,
            ok: true,
            data: json!({"text": "hi"}),
            error: None,
            duplicate: false,
        }
    );
    for (id, action, params, code) in [
        (2, "no.such", json!({}), error_code::UNKNOWN_ACTION),
        (3, "sh -c id", json!({}), error_code::UNKNOWN_ACTION),
        (4, "test.fail", json!({}), error_code::NOT_PERMITTED),
        (
            5,
            "test.echo",
            json!({"cmd": "x"}),
            error_code::INVALID_PARAMS,
        ),
        (
            6,
            "test.echo",
            json!({"text": 5}),
            error_code::INVALID_PARAMS,
        ),
    ] {
        let m = invoke(&mut c, id, action, params).await;
        assert!(matches!(m, AgentMessage::Result { id: i, .. } if i == id));
        assert_eq!(error_code_of(&m), code, "{action}");
    }
    // The session survives rejected requests.
    c.sender.send(&ClientMessage::Ping { id: 9 }).await.unwrap();
    assert_eq!(recv(&mut c).await.unwrap(), AgentMessage::Pong { id: 9 });
}

#[tokio::test]
async fn failing_action_reports_failure() {
    let phone = Identity::generate().unwrap();
    let a = agent(&phone, |_| {}).await;
    let (mut c, _) = session(&a, &phone).await;
    let m = invoke(&mut c, 1, "test.fail", json!({})).await;
    assert_eq!(error_code_of(&m), error_code::FAILED);
}

#[tokio::test]
async fn availability_changes_are_pushed() {
    let phone = Identity::generate().unwrap();
    let a = agent(&phone, |_| {}).await;
    let (mut c, _) = session(&a, &phone).await;
    a.toggle.store(false, Ordering::SeqCst);
    a.changes.notify();
    match recv(&mut c).await.unwrap() {
        AgentMessage::Actions { actions } => {
            let t = actions.iter().find(|a| a.id == "test.toggle").unwrap();
            assert!(!t.available);
            assert_eq!(t.reason, "switched off");
        }
        other => panic!("expected actions, got {other:?}"),
    }
    let m = invoke(&mut c, 1, "test.toggle", json!({})).await;
    assert_eq!(error_code_of(&m), error_code::UNAVAILABLE);
}

#[tokio::test]
async fn new_session_replaces_the_old_one() {
    let phone = Identity::generate().unwrap();
    let a = agent(&phone, |_| {}).await;
    let (mut old, _) = session(&a, &phone).await;
    let (mut new, _) = session(&a, &phone).await;
    let m = recv(&mut old).await.unwrap();
    assert_eq!(error_code_of(&m), error_code::REPLACED);
    assert!(recv(&mut old).await.is_none());
    new.sender
        .send(&ClientMessage::Ping { id: 1 })
        .await
        .unwrap();
    assert_eq!(recv(&mut new).await.unwrap(), AgentMessage::Pong { id: 1 });
}

#[tokio::test]
async fn silent_session_is_closed() {
    let phone = Identity::generate().unwrap();
    let a = agent_with(
        &phone,
        |_| {},
        |s| s.idle_timeout = Duration::from_millis(300),
    )
    .await;
    let (mut c, _) = session(&a, &phone).await;
    assert!(recv(&mut c).await.is_none(), "idle session must be closed");
}

#[tokio::test]
async fn garbage_instead_of_noise_is_dropped() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let phone = Identity::generate().unwrap();
    let a = agent(&phone, |_| {}).await;
    let mut s = TcpStream::connect(a.addr).await.unwrap();
    s.write_all(b"\x00\x10GET / HTTP/1.1\r\n\r\n")
        .await
        .unwrap();
    let mut buf = [0u8; 16];
    let n = timeout(WAIT, s.read(&mut buf)).await.unwrap().unwrap_or(0);
    assert_eq!(n, 0, "agent must close without answering");
}

// ── desktop.notify against a fake notification server on a private bus ─────────────

struct FakeNotifications {
    tx: tokio::sync::mpsc::UnboundedSender<(String, String, String)>,
}

#[zbus::interface(name = "org.freedesktop.Notifications")]
impl FakeNotifications {
    #[allow(clippy::too_many_arguments)]
    fn notify(
        &self,
        app_name: String,
        _replaces_id: u32,
        _app_icon: String,
        summary: String,
        body: String,
        _actions: Vec<String>,
        _hints: std::collections::HashMap<String, zbus::OwnedVariant>,
        _expire: i32,
    ) -> u32 {
        let _ = self.tx.send((app_name, summary, body));
        42
    }
}

struct Bus(std::process::Child, String);
impl Drop for Bus {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn private_bus() -> Bus {
    let mut d = std::process::Command::new("dbus-daemon")
        .args(["--session", "--nofork", "--nopidfile", "--print-address=1"])
        .stdout(std::process::Stdio::piped())
        .spawn()
        .expect("dbus-daemon must be installed");
    let mut line = String::new();
    std::io::BufRead::read_line(
        &mut std::io::BufReader::new(d.stdout.take().unwrap()),
        &mut line,
    )
    .unwrap();
    Bus(d, line.trim().to_owned())
}

#[tokio::test]
async fn desktop_notify_calls_the_notification_service() {
    let bus = private_bus();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let _server = zbus::connection::Builder::address(bus.1.as_str())
        .name("org.freedesktop.Notifications")
        .serve_at("/org/freedesktop/Notifications", FakeNotifications { tx })
        .build()
        .await
        .unwrap();
    let client = zbus::connection::Builder::address(bus.1.as_str())
        .build()
        .await
        .unwrap();

    let mut reg = Registry::new();
    let mpris = Mpris::start(client.clone(), Some(reg.changes()));
    pocket_desktop_agent::actions::builtin(&mut reg, desktop(Some(client), Some(mpris)));
    let p = |v: Value| v.as_object().unwrap().clone();
    let r = reg
        .invoke(
            "desktop.notify",
            &p(json!({"title": "T", "body": "<b>hi</b>"})),
        )
        .await
        .unwrap();
    assert_eq!(r, json!({"notification_id": 42}));
    let (app, summary, body) = timeout(WAIT, rx.recv()).await.unwrap().unwrap();
    assert_eq!((app.as_str(), summary.as_str()), ("Pocket OS", "T"));
    assert_eq!(body, "&lt;b&gt;hi&lt;/b&gt;", "markup must be escaped");

    let long = "x".repeat(121);
    let e = reg
        .invoke("desktop.notify", &p(json!({"title": long})))
        .await
        .unwrap_err();
    assert_eq!(e.code, error_code::INVALID_PARAMS);

    // No MPRIS player on this bus: play/pause is advertised as unavailable.
    tokio::time::sleep(Duration::from_millis(200)).await;
    let media = reg
        .list()
        .into_iter()
        .find(|a| a.id == "media.play_pause")
        .unwrap();
    assert!(!media.available);
    assert_eq!(
        reg.invoke("media.play_pause", &p(json!({})))
            .await
            .unwrap_err()
            .code,
        error_code::UNAVAILABLE
    );
}

// ── media.play_pause against a fake MPRIS player ─────────────────────────────────────

struct FakePlayer {
    toggled: tokio::sync::mpsc::UnboundedSender<()>,
}

#[zbus::interface(name = "org.mpris.MediaPlayer2.Player")]
impl FakePlayer {
    fn play_pause(&self) {
        let _ = self.toggled.send(());
    }
    #[zbus(property)]
    fn playback_status(&self) -> String {
        "Paused".into()
    }
}

#[tokio::test]
async fn media_play_pause_follows_players_on_the_bus() {
    let bus = private_bus();
    let client = zbus::connection::Builder::address(bus.1.as_str())
        .build()
        .await
        .unwrap();
    let mut reg = Registry::new();
    let mpris = Mpris::start(client.clone(), Some(reg.changes()));
    pocket_desktop_agent::actions::builtin(&mut reg, desktop(Some(client), Some(mpris.clone())));
    let available = |reg: &Registry| {
        reg.list()
            .into_iter()
            .find(|a| a.id == "media.play_pause")
            .unwrap()
            .available
    };
    tokio::time::sleep(Duration::from_millis(200)).await; // initial listing
    assert!(!available(&reg));

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let player = zbus::connection::Builder::address(bus.1.as_str())
        .name("org.mpris.MediaPlayer2.fake")
        .serve_at("/org/mpris/MediaPlayer2", FakePlayer { toggled: tx })
        .build()
        .await
        .unwrap();
    until(|| available(&reg)).await;
    let r = reg
        .invoke("media.play_pause", &Default::default())
        .await
        .unwrap();
    assert_eq!(r, json!({"player": "fake"}));
    timeout(WAIT, rx.recv()).await.unwrap().unwrap();

    drop(player);
    until(|| !available(&reg)).await;
    assert_eq!(mpris.player_count(), 0);
}

// ── Stage 4: helpers ──────────────────────────────────────────────────────────────────

fn desktop(bus: Option<zbus::Connection>, mpris: Option<Mpris>) -> Desktop {
    Desktop {
        bus,
        mpris,
        apps: Vec::new(),
    }
}

/// Waits (up to WAIT) until `f` holds.
async fn until(f: impl Fn() -> bool) {
    let start = std::time::Instant::now();
    while !f() {
        assert!(start.elapsed() < WAIT, "condition not reached in time");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// Counts executions; `delay_ms` makes it slow; elevated when `elevated`.
struct Counted {
    id: &'static str,
    runs: Arc<std::sync::atomic::AtomicU32>,
    delay_ms: u64,
    elevated: bool,
}

impl Action for Counted {
    fn info(&self) -> ActionInfo {
        ActionInfo {
            privilege: if self.elevated {
                pocket_dock_protocol::message::Privilege::Elevated
            } else {
                pocket_dock_protocol::message::Privilege::Basic
            },
            ..ActionInfo::new(self.id, self.id, "test")
        }
    }
    fn execute<'a>(&'a self, _params: &'a Params) -> ActionFuture<'a> {
        let n = self.runs.fetch_add(1, Ordering::SeqCst) + 1;
        Box::pin(async move {
            tokio::time::sleep(Duration::from_millis(self.delay_ms)).await;
            Ok(json!({ "run": n }))
        })
    }
}

fn counted(
    id: &'static str,
    delay_ms: u64,
    elevated: bool,
) -> (Counted, Arc<std::sync::atomic::AtomicU32>) {
    let runs = Arc::new(std::sync::atomic::AtomicU32::new(0));
    (
        Counted {
            id,
            runs: runs.clone(),
            delay_ms,
            elevated,
        },
        runs,
    )
}

// ── Stage 4: authorization, request keys, progress ───────────────────────────────────

#[tokio::test]
async fn elevated_actions_need_a_grant_over_the_wire() {
    let phone = Identity::generate().unwrap();
    let (act, runs) = counted("test.elevated", 0, true);
    let a = agent(&phone, move |r| r.register(act)).await;
    let (mut c, actions) = session(&a, &phone).await;
    let info = actions.iter().find(|x| x.id == "test.elevated").unwrap();
    assert!(!info.available && info.reason.contains("grant"), "{info:?}");
    assert_eq!(
        serde_json::to_value(info).unwrap()["privilege"],
        "elevated",
        "the phone sees the privilege class"
    );
    let m = invoke(&mut c, 1, "test.elevated", json!({})).await;
    assert_eq!(error_code_of(&m), error_code::NOT_PERMITTED);
    assert_eq!(runs.load(Ordering::SeqCst), 0);

    let (act, runs) = counted("test.elevated", 0, true);
    let a = agent(&phone, move |r| {
        r.register(act);
        r.set_grants(["test.elevated".to_owned()]);
    })
    .await;
    let (mut c, _) = session(&a, &phone).await;
    let m = invoke(&mut c, 1, "test.elevated", json!({})).await;
    assert!(matches!(m, AgentMessage::Result { ok: true, .. }), "{m:?}");
    assert_eq!(runs.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn a_repeated_request_key_returns_the_stored_result_across_sessions() {
    let phone = Identity::generate().unwrap();
    let (act, runs) = counted("test.count", 0, false);
    let a = agent(&phone, move |r| r.register(act)).await;
    let (mut c, _) = session(&a, &phone).await;
    let m = invoke_key(&mut c, 1, "test.count", json!({}), Some("key-1")).await;
    assert_eq!(
        m,
        AgentMessage::Result {
            id: 1,
            ok: true,
            data: json!({"run": 1}),
            error: None,
            duplicate: false
        }
    );
    drop(c); // the connection is lost; the phone does not know whether it ran

    let (mut c, _) = session(&a, &phone).await;
    let m = invoke_key(&mut c, 7, "test.count", json!({}), Some("key-1")).await;
    assert_eq!(
        m,
        AgentMessage::Result {
            id: 7,
            ok: true,
            data: json!({"run": 1}),
            error: None,
            duplicate: true
        },
        "same key: the stored result, under the new request id"
    );
    assert_eq!(runs.load(Ordering::SeqCst), 1, "executed exactly once");

    // Another key (a new intent) runs again; no key never de-duplicates.
    let m = invoke_key(&mut c, 8, "test.count", json!({}), Some("key-2")).await;
    assert!(matches!(
        m,
        AgentMessage::Result {
            duplicate: false,
            ..
        }
    ));
    invoke(&mut c, 9, "test.count", json!({})).await;
    invoke(&mut c, 10, "test.count", json!({})).await;
    assert_eq!(runs.load(Ordering::SeqCst), 4);
}

#[tokio::test]
async fn a_retry_racing_the_original_waits_for_it() {
    let phone = Identity::generate().unwrap();
    let (act, runs) = counted("test.slow", 1500, false);
    let a = agent(&phone, move |r| r.register(act)).await;
    let (mut c1, _) = session(&a, &phone).await;
    let sent = std::time::Instant::now();
    c1.sender
        .send(&ClientMessage::Invoke {
            id: 1,
            action: "test.slow".into(),
            params: Default::default(),
            key: Some("race".into()),
        })
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    // The phone reconnects (replacing the session) and retries while the original runs.
    let (mut c2, _) = session(&a, &phone).await;
    assert!(
        sent.elapsed() < Duration::from_millis(1500),
        "the retry must race the original"
    );
    let m = invoke_key(&mut c2, 2, "test.slow", json!({}), Some("race")).await;
    assert_eq!(
        m,
        AgentMessage::Result {
            id: 2,
            ok: true,
            data: json!({"run": 1}),
            error: None,
            duplicate: true
        }
    );
    assert!(
        sent.elapsed() >= Duration::from_millis(1500),
        "it waited for the original to finish"
    );
    assert_eq!(runs.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn workflow_progress_arrives_before_the_result() {
    let phone = Identity::generate().unwrap();
    let a = agent(&phone, |r| {
        r.set_grants(["workflow.run".to_owned()]);
        r.set_workflows(vec![pocket_desktop_agent::config::WorkflowConfig {
            id: "two".into(),
            label: "Two".into(),
            steps: vec![
                pocket_desktop_agent::config::WorkflowStep {
                    action: "test.echo".into(),
                    params: Default::default(),
                },
                pocket_desktop_agent::config::WorkflowStep {
                    action: "test.toggle".into(),
                    params: Default::default(),
                },
            ],
        }]);
    })
    .await;
    let (mut c, actions) = session(&a, &phone).await;
    assert!(
        actions
            .iter()
            .any(|x| x.id == "workflow.run" && x.available)
    );
    c.sender
        .send(&ClientMessage::Invoke {
            id: 5,
            action: "workflow.run".into(),
            params: json!({"workflow": "two"}).as_object().unwrap().clone(),
            key: None,
        })
        .await
        .unwrap();
    let mut seen = Vec::new();
    loop {
        match recv(&mut c).await.unwrap() {
            AgentMessage::Progress {
                id, step, total, ..
            } => seen.push((id, step, total)),
            AgentMessage::Result { id, ok, .. } => {
                assert_eq!((id, ok), (5, true));
                break;
            }
            _ => {}
        }
    }
    assert_eq!(seen, [(5, 1, 2), (5, 2, 2)]);
    // A workflow the laptop does not define is refused.
    let m = invoke(&mut c, 6, "workflow.run", json!({"workflow": "rm-rf"})).await;
    assert_eq!(error_code_of(&m), error_code::NOT_PERMITTED);
}

// ── Stage 4: live state and notifications ─────────────────────────────────────────────

async fn next_state(c: &mut Conn) -> (String, u64, bool, Value) {
    loop {
        if let AgentMessage::State {
            topic,
            rev,
            available,
            data,
            ..
        } = recv(c).await.unwrap()
        {
            return (topic, rev, available, data);
        }
    }
}

#[tokio::test]
async fn subscribe_gets_snapshots_then_coalesced_updates() {
    let phone = Identity::generate().unwrap();
    let a = agent(&phone, |_| {}).await;
    a.live.set("media", true, "", 0, json!({"active": "x"}));
    let (mut c, _) = session(&a, &phone).await;
    c.sender
        .send(&ClientMessage::Subscribe {
            topics: vec!["media".into(), "host".into(), "bogus".into()],
        })
        .await
        .unwrap();
    assert_eq!(
        next_state(&mut c).await,
        ("media".into(), 1, true, json!({"active": "x"}))
    );
    let (t, rev, available, _) = next_state(&mut c).await;
    assert_eq!(
        (t.as_str(), rev, available),
        ("host", 0, false),
        "unknown here: unavailable"
    );

    // Two quick changes arrive as one update with the latest value.
    a.live.set("media", true, "", 0, json!({"active": "y"}));
    a.live.set("media", true, "", 0, json!({"active": "z"}));
    assert_eq!(
        next_state(&mut c).await,
        ("media".into(), 3, true, json!({"active": "z"}))
    );
    // An unchanged value sends nothing; a later change does.
    a.live.set("media", true, "", 0, json!({"active": "z"}));
    a.live.set("host", true, "", 0, json!({"locked": false}));
    assert_eq!(next_state(&mut c).await.0, "host");
}

#[tokio::test]
async fn notifications_reach_only_subscribed_sessions() {
    let phone = Identity::generate().unwrap();
    let a = agent(&phone, |_| {}).await;
    let (mut c, _) = session(&a, &phone).await;
    c.sender
        .send(&ClientMessage::Subscribe {
            topics: vec!["notifications".into()],
        })
        .await
        .unwrap();
    next_state(&mut c).await; // the policy topic snapshot
    let note = pocket_dock_protocol::message::Notification {
        nid: "n1".into(),
        app: "Mail".into(),
        summary: "Hi".into(),
        body: String::new(),
        urgency: 1,
        redacted: true,
        time_ms: 1,
    };
    // The subscription is active once the snapshot was sent.
    a.notes
        .send(AgentMessage::Notification(note.clone()))
        .unwrap();
    loop {
        match recv(&mut c).await.unwrap() {
            AgentMessage::Notification(n) => {
                assert_eq!(n, note);
                break;
            }
            AgentMessage::State { .. } => continue,
            other => panic!("unexpected {other:?}"),
        }
    }
}

// ── Stage 4: desktop integrations against fake services ──────────────────────────────

struct FakeAccel {
    names: Vec<String>,
    invoked: tokio::sync::mpsc::UnboundedSender<String>,
}

#[zbus::interface(name = "org.kde.kglobalaccel.Component")]
impl FakeAccel {
    #[zbus(name = "shortcutNames")]
    fn shortcut_names(&self) -> Vec<String> {
        self.names.clone()
    }
    #[zbus(name = "invokeShortcut")]
    fn invoke_shortcut(&self, name: &str) {
        let _ = self.invoked.send(name.to_owned());
    }
}

struct FakeScreenSaver {
    locked: tokio::sync::mpsc::UnboundedSender<()>,
}

#[zbus::interface(name = "org.freedesktop.ScreenSaver")]
impl FakeScreenSaver {
    fn lock(&self) {
        let _ = self.locked.send(());
    }
    fn get_active(&self) -> bool {
        false
    }
}

#[tokio::test]
async fn kde_shortcuts_volume_and_lock_use_named_desktop_actions() {
    let bus = private_bus();
    let client = zbus::connection::Builder::address(bus.1.as_str())
        .build()
        .await
        .unwrap();
    let mut reg = Registry::new();
    pocket_desktop_agent::actions::builtin(&mut reg, desktop(Some(client.clone()), None));
    let find = |reg: &Registry, id: &str| reg.list().into_iter().find(|a| a.id == id).unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    // No KDE on this bus: unavailable, with a reason.
    assert!(!find(&reg, "volume.up").available);
    reg.set_grants(["desktop.shortcut".to_owned(), "session.lock".to_owned()]);
    assert!(!find(&reg, "desktop.shortcut").available);

    let (tx, mut invoked) = tokio::sync::mpsc::unbounded_channel();
    let (ltx, mut locked) = tokio::sync::mpsc::unbounded_channel();
    let kmix = ["increase_volume", "decrease_volume", "mute"]
        .map(String::from)
        .to_vec();
    let kwin = [
        "Overview",
        "Show Desktop",
        "Switch to Next Desktop",
        "Switch to Previous Desktop",
        "Grid View",
    ]
    .map(String::from)
    .to_vec();
    let _kde = zbus::connection::Builder::address(bus.1.as_str())
        .name("org.kde.kglobalaccel")
        .serve_at(
            "/component/kmix",
            FakeAccel {
                names: kmix,
                invoked: tx.clone(),
            },
        )
        .serve_at(
            "/component/kwin",
            FakeAccel {
                names: kwin,
                invoked: tx,
            },
        )
        .build()
        .await
        .unwrap();
    let _screensaver = zbus::connection::Builder::address(bus.1.as_str())
        .name("org.freedesktop.ScreenSaver")
        .serve_at(
            "/org/freedesktop/ScreenSaver",
            FakeScreenSaver { locked: ltx },
        )
        .build()
        .await
        .unwrap();
    until(|| find(&reg, "volume.up").available).await;
    let p = |v: Value| v.as_object().unwrap().clone();
    reg.invoke("volume.up", &p(json!({}))).await.unwrap();
    assert_eq!(
        timeout(WAIT, invoked.recv()).await.unwrap().unwrap(),
        "increase_volume"
    );

    let sc = find(&reg, "desktop.shortcut");
    assert!(sc.available);
    let choices: Vec<_> = sc.params[0]
        .choices
        .iter()
        .map(|c| c.value.as_str())
        .collect();
    assert!(choices.contains(&"overview") && choices.contains(&"desktop_next"));
    reg.invoke("desktop.shortcut", &p(json!({"shortcut": "desktop_next"})))
        .await
        .unwrap();
    assert_eq!(
        timeout(WAIT, invoked.recv()).await.unwrap().unwrap(),
        "Switch to Next Desktop"
    );
    let e = reg
        .invoke("desktop.shortcut", &p(json!({"shortcut": "Window Close"})))
        .await
        .unwrap_err();
    assert_eq!(e.code, error_code::NOT_PERMITTED, "only approved shortcuts");

    let lock = find(&reg, "session.lock");
    assert!(lock.confirm && lock.idempotent);
    reg.invoke("session.lock", &p(json!({}))).await.unwrap();
    timeout(WAIT, locked.recv()).await.unwrap().unwrap();
}

// A richer MPRIS player: metadata and PropertiesChanged.
struct RichPlayer {
    status: String,
    title: String,
}

#[zbus::interface(name = "org.mpris.MediaPlayer2.Player")]
impl RichPlayer {
    #[zbus(property)]
    fn playback_status(&self) -> String {
        self.status.clone()
    }
    #[zbus(property)]
    fn metadata(&self) -> std::collections::HashMap<String, zbus::OwnedVariant> {
        let mut m = std::collections::HashMap::new();
        m.insert(
            "xesam:title".to_owned(),
            zbus::Variant::from(self.title.clone()).try_into().unwrap(),
        );
        m.insert(
            "xesam:artist".to_owned(),
            zbus::Variant::from(vec!["Artist".to_owned()])
                .try_into()
                .unwrap(),
        );
        m
    }
    #[zbus(property)]
    fn can_go_next(&self) -> bool {
        true
    }
}

struct Root;
#[zbus::interface(name = "org.mpris.MediaPlayer2")]
impl Root {
    #[zbus(property)]
    fn identity(&self) -> String {
        "Rich Player".into()
    }
}

#[tokio::test]
async fn media_state_follows_player_properties() {
    let bus = private_bus();
    let client = zbus::connection::Builder::address(bus.1.as_str())
        .build()
        .await
        .unwrap();
    let mpris = Mpris::start(client, None);
    let player = zbus::connection::Builder::address(bus.1.as_str())
        .name("org.mpris.MediaPlayer2.rich")
        .serve_at(
            "/org/mpris/MediaPlayer2",
            RichPlayer {
                status: "Paused".into(),
                title: "One".into(),
            },
        )
        .serve_at("/org/mpris/MediaPlayer2", Root)
        .build()
        .await
        .unwrap();
    until(|| {
        mpris.snapshot()["players"]
            .as_array()
            .is_some_and(|p| !p.is_empty())
    })
    .await;
    let s = mpris.snapshot();
    assert_eq!(s["active"], "rich");
    assert_eq!(s["players"][0]["title"], "One");
    assert_eq!(s["players"][0]["artist"], "Artist");
    assert_eq!(s["players"][0]["identity"], "Rich Player");
    assert_eq!(s["players"][0]["can_next"], true);

    // The player starts playing a new track and says so with PropertiesChanged.
    let iface = player
        .object_server()
        .interface::<_, RichPlayer>("/org/mpris/MediaPlayer2")
        .await
        .unwrap();
    {
        let mut p = iface.get_mut().await;
        p.status = "Playing".into();
        p.title = "Two".into();
        p.playback_status_changed(iface.signal_emitter())
            .await
            .unwrap();
    }
    until(|| mpris.snapshot()["players"][0]["title"] == "Two").await;
    assert_eq!(mpris.snapshot()["players"][0]["status"], "Playing");
}

// The notification monitor against a fake notification server on a private bus.
struct FakeServer;

#[zbus::interface(name = "org.freedesktop.Notifications")]
impl FakeServer {
    #[allow(clippy::too_many_arguments)]
    fn notify(
        &self,
        _app: &str,
        _replaces: u32,
        _icon: &str,
        _summary: &str,
        _body: &str,
        _actions: Vec<String>,
        _hints: std::collections::HashMap<String, zbus::OwnedVariant>,
        _timeout: i32,
    ) -> u32 {
        77
    }
    #[zbus(signal)]
    async fn notification_closed(
        e: &zbus::object_server::SignalEmitter<'_>,
        id: u32,
        reason: u32,
    ) -> zbus::Result<()>;
}

#[tokio::test]
async fn the_monitor_forwards_notifications_by_policy_and_reports_closing() {
    let bus = private_bus();
    let server = zbus::connection::Builder::address(bus.1.as_str())
        .name("org.freedesktop.Notifications")
        .serve_at("/org/freedesktop/Notifications", FakeServer)
        .build()
        .await
        .unwrap();
    let live = pocket_desktop_agent::live::Live::new();
    live.set("host", true, "", 0, json!({"locked": false}));
    let (tx, mut rx) = tokio::sync::broadcast::channel(8);
    let policy = pocket_desktop_agent::notifications::Policy(
        pocket_desktop_agent::config::NotificationPolicy {
            forward: true,
            deny_apps: vec!["Noisy".into()],
            ..Default::default()
        },
    );
    pocket_desktop_agent::notifications::start_on(Some(bus.1.clone()), policy, live.clone(), tx);
    until(|| live.data("notifications")["forwarding"] == true).await;

    let app = zbus::connection::Builder::address(bus.1.as_str())
        .build()
        .await
        .unwrap();
    let send = |name: &'static str, body: &'static str| {
        let app = app.clone();
        async move {
            app.call_method(
                Some("org.freedesktop.Notifications"),
                "/org/freedesktop/Notifications",
                Some("org.freedesktop.Notifications"),
                "Notify",
                &(
                    name,
                    0u32,
                    "",
                    "Subject",
                    body,
                    Vec::<&str>::new(),
                    std::collections::HashMap::<&str, zbus::Variant<'_>>::new(),
                    -1i32,
                ),
            )
            .await
            .unwrap();
        }
    };
    send("Noisy", "ignored").await;
    send("Pocket OS", "our own echo").await;
    send("Mail", "<b>hello</b>").await;
    let n = match timeout(WAIT, rx.recv()).await.unwrap().unwrap() {
        AgentMessage::Notification(n) => n,
        other => panic!("{other:?}"),
    };
    assert_eq!(
        (n.nid.as_str(), n.app.as_str(), n.body.as_str()),
        ("n77", "Mail", "hello")
    );

    let e =
        zbus::object_server::SignalEmitter::new(&server, "/org/freedesktop/Notifications").unwrap();
    FakeServer::notification_closed(&e, 77, 2).await.unwrap();
    match timeout(WAIT, rx.recv()).await.unwrap().unwrap() {
        AgentMessage::NotificationClosed { nid } => assert_eq!(nid, "n77"),
        other => panic!("{other:?}"),
    }
}

// ── app.launch against a private desktop-entry catalog ───────────────────────────────

#[tokio::test]
async fn app_launch_starts_only_registered_installed_apps_without_a_shell() {
    use pocket_desktop_agent::actions::apps::AppLaunch;
    use pocket_platform::apps::{Catalog, ScanOptions};
    let dir = std::env::temp_dir().join(format!("pocket-agent-apps-{}", std::process::id()));
    let apps = dir.join("applications");
    std::fs::create_dir_all(&apps).unwrap();
    // Exec runs `touch <dir>/a;b`: with no shell, `a;b` is one literal file name (a shell
    // would split it into a command `touch <dir>/a` and a command `b`).
    let marker = dir.join("a;b");
    std::fs::write(
        apps.join("test.app.desktop"),
        format!(
            "[Desktop Entry]\nType=Application\nName=Test App\nExec=touch \"{}\"\n",
            marker.display()
        ),
    )
    .unwrap();
    std::fs::write(
        apps.join("other.app.desktop"),
        "[Desktop Entry]\nType=Application\nName=Other\nExec=true\n",
    )
    .unwrap();
    let opts = ScanOptions {
        dirs: vec![apps.clone()],
        desktops: vec![],
        locales: vec![],
        path: std::env::split_paths(&std::env::var_os("PATH").unwrap()).collect(),
        resolve_icons: false,
        icon_theme: String::new(),
    };
    let catalog = Catalog::scan(&opts).0;
    assert!(catalog.get("test.app").is_some(), "test entry is valid");

    let mut reg = Registry::new();
    let changes = reg.changes();
    reg.register(AppLaunch::with_catalog(
        vec!["test.app".into(), "missing.app".into()],
        catalog,
        changes,
    ));
    let info = reg
        .list()
        .into_iter()
        .find(|a| a.id == "app.launch")
        .unwrap();
    assert!(!info.available, "elevated: not granted yet");
    let choices: Vec<_> = info.params[0]
        .choices
        .iter()
        .map(|c| (c.value.as_str(), c.label.as_str()))
        .collect();
    assert_eq!(
        choices,
        [("test.app", "Test App")],
        "only registered AND installed apps are offered"
    );

    reg.set_grants(["app.launch".to_owned()]);
    let p = |v: Value| v.as_object().unwrap().clone();
    let r = reg
        .invoke("app.launch", &p(json!({"app": "test.app"})))
        .await
        .unwrap();
    assert_eq!(r["app"], "test.app");
    until(|| marker.exists()).await;
    for bad in ["other.app", "/usr/bin/true", "missing.app"] {
        let e = reg
            .invoke("app.launch", &p(json!({"app": bad})))
            .await
            .unwrap_err();
        assert_eq!(e.code, error_code::NOT_PERMITTED, "{bad}");
    }
    assert!(!dir.join("a").exists(), "no shell split the argument");
    std::fs::remove_dir_all(&dir).unwrap();
}

// ── Stage 4.5: live configuration, destinations, laptop-approved access requests ──────

/// A notification server that hands out increasing ids and records the actions offered.
struct FakePrompter {
    next: std::sync::atomic::AtomicU32,
    tx: tokio::sync::mpsc::UnboundedSender<(u32, Vec<String>)>,
}

#[zbus::interface(name = "org.freedesktop.Notifications")]
impl FakePrompter {
    #[allow(clippy::too_many_arguments)]
    fn notify(
        &self,
        _app_name: String,
        _replaces_id: u32,
        _app_icon: String,
        _summary: String,
        _body: String,
        actions: Vec<String>,
        _hints: std::collections::HashMap<String, zbus::OwnedVariant>,
        _expire: i32,
    ) -> u32 {
        let id = self.next.fetch_add(1, Ordering::SeqCst);
        let _ = self.tx.send((id, actions));
        id
    }
    fn close_notification(&self, _id: u32) {}
}

fn temp_config(name: &str) -> (std::path::PathBuf, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("pocket-agent-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("config.json");
    let cfg: pocket_desktop_agent::config::Config =
        serde_json::from_value(json!({"listen": "127.0.0.1:1"})).unwrap();
    cfg.save(&path).unwrap();
    (dir, path)
}

#[tokio::test]
async fn access_requests_are_decided_on_the_laptop_only() {
    use pocket_desktop_agent::actions::access::AccessConfig;
    use pocket_desktop_agent::config::Config;
    let bus = private_bus();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let server = zbus::connection::Builder::address(bus.1.as_str())
        .name("org.freedesktop.Notifications")
        .serve_at(
            "/org/freedesktop/Notifications",
            FakePrompter { next: 7.into(), tx },
        )
        .build()
        .await
        .unwrap();
    let client = zbus::connection::Builder::address(bus.1.as_str())
        .build()
        .await
        .unwrap();
    let (dir, path) = temp_config("access");

    let mut reg = Registry::new();
    pocket_desktop_agent::actions::builtin(&mut reg, desktop(Some(client.clone()), None));
    reg.set_access(AccessConfig {
        bus: client,
        config_path: Some(path.clone()),
        host_name: "test".into(),
    });
    let reg = Arc::new(reg);
    let info = |id: &str| reg.list().into_iter().find(|a| a.id == id).unwrap();
    let ask = info("access.request");
    assert!(ask.available, "{}", ask.reason);
    assert_eq!(
        ask.privilege,
        pocket_dock_protocol::message::Privilege::Basic
    );
    let choices: Vec<String> = ask.params[0]
        .choices
        .iter()
        .map(|c| c.value.clone())
        .collect();
    assert!(choices.contains(&"session.lock".to_owned()), "{choices:?}");
    assert!(
        !choices.contains(&"media.next".to_owned()),
        "basic actions are not requestable"
    );

    let p = |v: Value| v.as_object().unwrap().clone();
    // Only elevated actions can be asked for.
    let e = reg
        .invoke("access.request", &p(json!({"action": "media.next"})))
        .await
        .unwrap_err();
    assert_eq!(e.code, error_code::NOT_PERMITTED);

    // Ask → prompt with Allow/Deny on the laptop; nothing is granted yet.
    let r = reg
        .invoke("access.request", &p(json!({"action": "session.lock"})))
        .await
        .unwrap();
    assert_eq!(r["status"], "asked");
    let (nid, actions) = timeout(WAIT, rx.recv()).await.unwrap().unwrap();
    assert_eq!(actions, ["deny", "Deny", "allow", "Allow"]);
    assert!(!info("session.lock").available, "asking grants nothing");
    // One open prompt at a time.
    let e = reg
        .invoke("access.request", &p(json!({"action": "desktop.shortcut"})))
        .await
        .unwrap_err();
    assert_eq!(e.code, error_code::UNAVAILABLE);

    // The laptop owner clicks Allow: granted now and saved.
    let mut changes = reg.subscribe();
    changes.mark_unchanged();
    server
        .emit_signal(
            None::<()>,
            "/org/freedesktop/Notifications",
            "org.freedesktop.Notifications",
            "ActionInvoked",
            &(nid, "allow"),
        )
        .await
        .unwrap();
    until(|| reg.is_granted("session.lock")).await;
    timeout(WAIT, changes.changed()).await.unwrap().unwrap();
    assert!(!info("session.lock").reason.contains("not granted"));
    assert!(
        Config::load(&path)
            .unwrap()
            .grants
            .contains(&"session.lock".to_owned())
    );
    assert!(
        !info("access.request").params[0]
            .choices
            .iter()
            .any(|c| c.value == "session.lock"),
        "granted actions are no longer requestable"
    );

    // Deny: nothing granted, and the same request is refused for a while.
    reg.invoke("access.request", &p(json!({"action": "desktop.shortcut"})))
        .await
        .unwrap();
    let (nid, _) = timeout(WAIT, rx.recv()).await.unwrap().unwrap();
    server
        .emit_signal(
            None::<()>,
            "/org/freedesktop/Notifications",
            "org.freedesktop.Notifications",
            "ActionInvoked",
            &(nid, "deny"),
        )
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(!reg.is_granted("desktop.shortcut"));
    let e = reg
        .invoke("access.request", &p(json!({"action": "desktop.shortcut"})))
        .await
        .unwrap_err();
    assert_eq!(e.code, error_code::UNAVAILABLE);
    assert!(e.message.contains("denied"), "{}", e.message);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[tokio::test]
async fn config_reload_and_destinations_apply_without_a_restart() {
    use pocket_desktop_agent::config::{Config, Destination};
    let dir = std::env::temp_dir().join(format!("pocket-agent-dest-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let apps = dir.join("applications");
    std::fs::create_dir_all(&apps).unwrap();
    let marker = dir.join("opened");
    let script = dir.join("opener");
    std::fs::write(
        &script,
        format!("#!/bin/sh\necho \"$@\" > '{}'\n", marker.display()),
    )
    .unwrap();
    std::fs::set_permissions(&script, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    std::fs::write(
        apps.join("test.editor.desktop"),
        format!(
            "[Desktop Entry]\nType=Application\nName=Editor\nExec={} %F\n",
            script.display()
        ),
    )
    .unwrap();
    let project = dir.join("my project");
    std::fs::create_dir_all(&project).unwrap();
    let opts = pocket_platform::apps::ScanOptions {
        dirs: vec![apps.clone()],
        desktops: vec![],
        locales: vec![],
        path: std::env::split_paths(&std::env::var_os("PATH").unwrap()).collect(),
        resolve_icons: false,
        icon_theme: String::new(),
    };
    let catalog = pocket_platform::apps::Catalog::scan(&opts).0;
    let mut reg = Registry::new();
    reg.register(
        pocket_desktop_agent::actions::destinations::OpenDestination::new(
            reg.lists(),
            Arc::new(std::sync::Mutex::new(Some(catalog))),
        ),
    );
    let reg = Arc::new(reg);
    let info = |id: &str| reg.list().into_iter().find(|a| a.id == id).unwrap();
    let open = info("desktop.open");
    assert!(!open.available);
    assert_eq!(
        open.privilege,
        pocket_dock_protocol::message::Privilege::Elevated
    );

    let mut cfg: Config = serde_json::from_value(json!({"listen": "127.0.0.1:1"})).unwrap();
    cfg.grants = vec!["desktop.open".into()];
    cfg.destinations = vec![Destination {
        id: "proj".into(),
        label: "My project".into(),
        app: "test.editor".into(),
        target: project.to_string_lossy().into_owned(),
    }];
    let mut changes = reg.subscribe();
    changes.mark_unchanged();
    reg.apply_config(&cfg);
    timeout(WAIT, changes.changed()).await.unwrap().unwrap();
    let open = info("desktop.open");
    assert!(open.available, "{}", open.reason);
    assert_eq!(open.params[0].choices[0].label, "My project");

    let p = |v: Value| v.as_object().unwrap().clone();
    let r = reg
        .invoke("desktop.open", &p(json!({"destination": "proj"})))
        .await
        .unwrap();
    assert_eq!(r["app"], "test.editor");
    until(|| marker.exists()).await;
    assert_eq!(
        std::fs::read_to_string(&marker).unwrap().trim_end(),
        project.to_string_lossy(),
        "the configured target, as one argument, through the entry's %F"
    );
    for bad in ["other", "/etc/passwd"] {
        let e = reg
            .invoke("desktop.open", &p(json!({"destination": bad})))
            .await
            .unwrap_err();
        assert_eq!(e.code, error_code::NOT_PERMITTED, "{bad}");
    }

    // Revoking through a reload takes effect immediately.
    cfg.grants.clear();
    reg.apply_config(&cfg);
    let e = reg
        .invoke("desktop.open", &p(json!({"destination": "proj"})))
        .await
        .unwrap_err();
    assert_eq!(e.code, error_code::NOT_PERMITTED);
    std::fs::remove_dir_all(&dir).unwrap();
}
