//! Black-box contract tests: run the real `pocketd` binary on a private `dbus-daemon`
//! session bus and talk to it as an ordinary D-Bus client would.
//!
//! Requires `dbus-daemon` in PATH (package `dbus` on Fedora/Alpine). On the device:
//! `scripts/test-device.sh` (cross-built test binary, `POCKETD_BIN` = deployed daemon).

use std::collections::HashMap;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use futures_util::StreamExt;
use tokio::time::timeout;
use zbus::{Connection, OwnedVariant, proxy};

#[proxy(
    interface = "org.pocketos.Core1",
    default_service = "org.pocketos.Core1",
    default_path = "/org/pocketos/Core1"
)]
trait Core1 {
    fn get_version(&self) -> zbus::Result<(String, u32, u32)>;
    fn get_state(&self) -> zbus::Result<HashMap<String, OwnedVariant>>;
    fn get_mode(&self) -> zbus::Result<String>;
    fn request_mode(&self, mode: &str) -> zbus::Result<()>;
    #[zbus(signal)]
    fn mode_changed(&self, mode: String) -> zbus::Result<()>;
    #[zbus(signal)]
    fn state_changed(&self, state: HashMap<String, OwnedVariant>) -> zbus::Result<()>;
}

#[proxy(
    interface = "org.pocketos.Core1.Apps",
    default_service = "org.pocketos.Core1",
    default_path = "/org/pocketos/Core1/Apps"
)]
trait Apps {
    fn list_apps(&self) -> zbus::Result<Vec<HashMap<String, OwnedVariant>>>;
    fn launch(&self, id: &str) -> zbus::Result<u32>;
    fn reload(&self) -> zbus::Result<()>;
    #[zbus(signal)]
    fn apps_changed(&self) -> zbus::Result<()>;
    #[zbus(signal)]
    fn app_exited(&self, id: String, pid: u32, success: bool, detail: String) -> zbus::Result<()>;
}

const WAIT: Duration = Duration::from_secs(5);

/// A private session bus, killed on drop.
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
            .expect("dbus-daemon must be installed to run the contract tests");
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

    fn spawn_core(&self) -> Child {
        self.spawn_core_with(&[])
    }

    fn spawn_core_with(&self, env: &[(&str, &str)]) -> Child {
        // POCKETD_BIN: run a cross-built test binary on the device against a deployed pocketd.
        let bin = std::env::var("POCKETD_BIN").unwrap_or(env!("CARGO_BIN_EXE_pocketd").into());
        Command::new(bin)
            .envs(env.iter().copied())
            .env("DBUS_SESSION_BUS_ADDRESS", &self.address)
            .env("POCKETD_LOG", "debug")
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

async fn has_owner(conn: &Connection) -> bool {
    zbus::fdo::DBusProxy::new(conn)
        .await
        .unwrap()
        .name_has_owner("org.pocketos.Core1".try_into().unwrap())
        .await
        .unwrap()
}

/// Waits until the core owns its name (it marks itself ready right after).
async fn wait_ready(conn: &Connection) {
    timeout(WAIT, async {
        while !has_owner(conn).await {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("pocketd did not register its name");
}

fn get<T: TryFrom<OwnedVariant>>(state: &HashMap<String, OwnedVariant>, key: &str) -> T
where
    T::Error: std::fmt::Debug,
{
    T::try_from(state[key].try_clone().unwrap()).unwrap()
}

fn sigterm(child: &Child) {
    let ok = Command::new("kill")
        .args(["-TERM", &child.id().to_string()])
        .status()
        .unwrap()
        .success();
    assert!(ok);
}

fn wait_exit(child: &mut Child) -> std::process::ExitStatus {
    for _ in 0..250 {
        if let Some(status) = child.try_wait().unwrap() {
            return status;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let _ = child.kill();
    panic!("pocketd did not exit");
}

#[tokio::test]
async fn contract_state_signals_errors_and_restart() {
    let bus = Bus::start();
    let mut core = bus.spawn_core();
    let conn = bus.client().await;
    wait_ready(&conn).await;
    let proxy = Core1Proxy::new(&conn).await.unwrap();

    // Initial state.
    let (version, major, minor) = proxy.get_version().await.unwrap();
    assert_eq!(version, env!("CARGO_PKG_VERSION"));
    assert_eq!((major, minor), (1, 1));
    let state = proxy.get_state().await.unwrap();
    assert_eq!(get::<String>(&state, "mode"), "handheld");
    assert_eq!(get::<String>(&state, "protocol"), "1.1");
    assert!(get::<bool>(&state, "ready"));
    let rev0: u64 = get(&state, "revision");
    assert_eq!(proxy.get_mode().await.unwrap(), "handheld");

    // Two independent subscribers (separate connections) both see the change.
    let conn2 = bus.client().await;
    let proxy2 = Core1Proxy::new(&conn2).await.unwrap();
    let mut modes1 = proxy.receive_mode_changed().await.unwrap();
    let mut modes2 = proxy2.receive_mode_changed().await.unwrap();
    let mut states2 = proxy2.receive_state_changed().await.unwrap();

    proxy.request_mode("dock").await.unwrap();
    for stream in [&mut modes1, &mut modes2] {
        let sig = timeout(WAIT, stream.next()).await.unwrap().unwrap();
        assert_eq!(sig.args().unwrap().mode, "dock");
    }
    let sig = timeout(WAIT, states2.next()).await.unwrap().unwrap();
    let args = sig.args().unwrap();
    assert_eq!(get::<String>(&args.state, "mode"), "dock");
    assert_eq!(get::<u64>(&args.state, "revision"), rev0 + 1);
    assert_eq!(proxy2.get_mode().await.unwrap(), "dock");

    // Same mode: success, no signal.
    proxy.request_mode("dock").await.unwrap();
    assert!(
        timeout(Duration::from_millis(300), modes1.next())
            .await
            .is_err()
    );

    // Invalid input: typed D-Bus error, state unchanged.
    let err = proxy.request_mode("desktop").await.unwrap_err();
    match err {
        zbus::Error::MethodError(name, msg, _) => {
            assert_eq!(name.as_str(), "org.pocketos.Core1.Error.InvalidMode");
            assert!(msg.unwrap().contains("desktop"));
        }
        other => panic!("unexpected error {other:?}"),
    }
    assert_eq!(proxy.get_mode().await.unwrap(), "dock");

    // A second core refuses to start while the first owns the name.
    let mut second = bus.spawn_core();
    assert_eq!(wait_exit(&mut second).code(), Some(3));
    assert_eq!(proxy.get_mode().await.unwrap(), "dock");

    // Graceful shutdown releases the name.
    sigterm(&core);
    assert!(wait_exit(&mut core).success());
    assert!(!has_owner(&conn).await);
    assert!(proxy.get_mode().await.is_err());

    // Restart: same proxy works again, fresh in-memory state.
    let mut core = bus.spawn_core();
    wait_ready(&conn).await;
    assert_eq!(proxy.get_mode().await.unwrap(), "handheld");
    sigterm(&core);
    assert!(wait_exit(&mut core).success());
}

/// A temporary XDG data tree: `<root>/home/applications`, `<root>/system/applications`.
struct DataDirs(std::path::PathBuf);

impl DataDirs {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("pocketd-contract-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for d in ["home/applications", "system/applications"] {
            std::fs::create_dir_all(root.join(d)).unwrap();
        }
        DataDirs(root)
    }
    fn path(&self, rel: &str) -> String {
        self.0.join(rel).to_string_lossy().into_owned()
    }
    fn write(&self, rel: &str, body: &str) {
        let tmp = self.0.join(format!("{rel}.tmp"));
        std::fs::write(&tmp, body).unwrap();
        std::fs::rename(tmp, self.0.join(rel)).unwrap();
    }
}

impl Drop for DataDirs {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[tokio::test]
async fn contract_apps_catalog_launch_and_exit() {
    let data = DataDirs::new();
    data.write(
        "system/applications/test.fails.desktop",
        "[Desktop Entry]\nType=Application\nName=Fails\nExec=sh -c \"exit 3\"\n",
    );
    data.write(
        "system/applications/test.ok.desktop",
        "[Desktop Entry]\nType=Application\nName=Alright\nIcon=/nonexistent.png\n\
         StartupWMClass=ok-app\nCategories=Utility;\nExec=true\n",
    );
    data.write(
        "system/applications/test.hidden.desktop",
        "[Desktop Entry]\nType=Application\nName=Hidden\nNoDisplay=true\nExec=true\n",
    );
    let bus = Bus::start();
    let home = data.path("home");
    let system = data.path("system");
    let mut core = bus.spawn_core_with(&[
        ("XDG_DATA_HOME", home.as_str()),
        ("XDG_DATA_DIRS", system.as_str()),
        ("XDG_CURRENT_DESKTOP", "Pocket"),
    ]);
    let conn = bus.client().await;
    wait_ready(&conn).await;
    let apps = AppsProxy::new(&conn).await.unwrap();

    // Catalog: visible entries only, sorted by name, documented keys.
    let list = apps.list_apps().await.unwrap();
    let ids: Vec<String> = list.iter().map(|a| get(a, "id")).collect();
    assert_eq!(ids, ["test.ok", "test.fails"]);
    let ok = &list[0];
    assert_eq!(get::<String>(ok, "name"), "Alright");
    assert_eq!(get::<String>(ok, "icon_path"), ""); // missing file: not resolved
    assert_eq!(get::<String>(ok, "icon_name"), "/nonexistent.png");
    assert_eq!(get::<String>(ok, "startup_wm_class"), "ok-app");
    assert_eq!(get::<Vec<String>>(ok, "categories"), ["Utility"]);

    // Launch + exit reporting (failure is visible to clients).
    let mut exits = apps.receive_app_exited().await.unwrap();
    let pid = apps.launch("test.fails").await.unwrap();
    assert!(pid > 0);
    let sig = timeout(WAIT, exits.next()).await.unwrap().unwrap();
    let a = sig.args().unwrap();
    assert_eq!(
        (a.id.as_str(), a.pid, a.success),
        ("test.fails", pid, false)
    );
    assert_eq!(a.detail, "status=3");
    let pid = apps.launch("test.ok").await.unwrap();
    let sig = timeout(WAIT, exits.next()).await.unwrap().unwrap();
    let a = sig.args().unwrap();
    assert_eq!(
        (a.pid, a.success, a.detail.as_str()),
        (pid, true, "status=0")
    );

    // Unknown and hidden apps are rejected with a typed error.
    for id in ["nope", "test.hidden"] {
        match apps.launch(id).await.unwrap_err() {
            zbus::Error::MethodError(name, _, _) => {
                assert_eq!(name.as_str(), "org.pocketos.Core1.Error.UnknownApp")
            }
            other => panic!("unexpected error {other:?}"),
        }
    }

    // A new desktop file is picked up without polling (inotify) and announced.
    let mut changes = apps.receive_apps_changed().await.unwrap();
    data.write(
        "system/applications/test.new.desktop",
        "[Desktop Entry]\nType=Application\nName=Brand New\nExec=true\n",
    );
    timeout(WAIT, changes.next()).await.unwrap().unwrap();
    let ids: Vec<String> = apps
        .list_apps()
        .await
        .unwrap()
        .iter()
        .map(|a| get(a, "id"))
        .collect();
    assert_eq!(ids, ["test.ok", "test.new", "test.fails"]);
    // Reload with nothing changed: success, no signal.
    apps.reload().await.unwrap();
    assert!(
        timeout(Duration::from_millis(300), changes.next())
            .await
            .is_err()
    );

    sigterm(&core);
    assert!(wait_exit(&mut core).success());
}
