//! Protocol tests: a real agent `Server` on 127.0.0.1, real TCP + Noise, a raw client.
//! The desktop-notification test also runs a private `dbus-daemon` with a fake
//! notification server, so nothing reaches the real desktop.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use pocket_desktop_agent::actions::{Action, ActionFuture, Changes, Params, Registry};
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
            id: self.id.into(),
            label: self.id.into(),
            category: "test".into(),
            available,
            reason: if available {
                String::new()
            } else {
                "switched off".into()
            },
            params: vec![pocket_dock_protocol::message::ParamInfo {
                name: "text".into(),
                kind: pocket_dock_protocol::message::ParamKind::String,
                required: false,
            }],
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
    c.sender
        .send(&ClientMessage::Invoke {
            id,
            action: action.into(),
            params: params.as_object().unwrap().clone(),
        })
        .await
        .unwrap();
    loop {
        match recv(c).await.unwrap() {
            AgentMessage::Actions { .. } => continue,
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
            error: None
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
    pocket_desktop_agent::actions::builtin(&mut reg, Some(client));
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
    pocket_desktop_agent::actions::builtin(&mut reg, Some(client));
    let mut changed = reg.subscribe();
    let available = |reg: &Registry| {
        reg.list()
            .into_iter()
            .find(|a| a.id == "media.play_pause")
            .unwrap()
            .available
    };
    timeout(WAIT, changed.changed()).await.unwrap().unwrap(); // initial listing
    assert!(!available(&reg));

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let player = zbus::connection::Builder::address(bus.1.as_str())
        .name("org.mpris.MediaPlayer2.fake")
        .serve_at("/org/mpris/MediaPlayer2", FakePlayer { toggled: tx })
        .build()
        .await
        .unwrap();
    timeout(WAIT, changed.changed()).await.unwrap().unwrap();
    assert!(available(&reg));
    let r = reg
        .invoke("media.play_pause", &Default::default())
        .await
        .unwrap();
    assert_eq!(r, json!({"player": "fake"}));
    timeout(WAIT, rx.recv()).await.unwrap().unwrap();

    drop(player);
    timeout(WAIT, changed.changed()).await.unwrap().unwrap();
    assert!(!available(&reg));
}
