pragma Singleton
import QtQuick
import Pocket.Core
import Pocket.Platform

// Shared shell state and actions: the one place UI components get Pocket Core, system
// adapters and navigation from. Portable Qt Quick (no Quickshell types): compositor-
// and service-specific adapters are injected by the Quickshell layer (shell.qml):
//
//   Shell.windows  window list/activation (services/WindowService.qml: wlr-foreign-toplevel)
//   Shell.system   battery/network/bluetooth (services/SystemStatus.qml)
//
// State ownership (docs/decisions/0004-shell-ui-architecture.md):
//   core-owned        core.mode, app catalog + launching (pocketd)
//   compositor-owned  windows, focus, output power/transform (Phoc; mirrored here)
//   system-owned      battery, network, bluetooth, backlight, sensors (their services)
//   UI-owned          homeVisible, homePage, panel, rotationMode, toasts (this file)
QtObject {
    id: shell

    // ── Core (one connection per shell process) ──────────────────────────────
    readonly property CoreClient core: CoreClient {
        onStatusChanged: shell.diag("core.status", { status: statusText.toLowerCase(),
            owner: owner || "-", connections: connections,
            error: lastError ? JSON.stringify(lastError) : "-" })
        onModeChanged: shell.diag("core.mode", { mode: mode, revision: revision })
        onRequestFinished: (mode, ok, error) => shell.diag("core.request", {
            mode: mode, ok: ok, error: ok ? "-" : JSON.stringify(error) })
    }
    readonly property bool coreConnected: core.status === CoreClient.Connected

    readonly property AppCatalog apps: AppCatalog {
        core: shell.core
        onLaunched: (appId, pid) => shell.diag("app.launched", { id: appId, pid: pid })
        onLaunchFailed: (appId, error) => {
            shell.diag("app.failed", { id: appId, error: JSON.stringify(error) })
            delete shell._pending[appId]
            shell.notify(shell.appName(appId) + " could not be started: " + error, "error")
        }
        onExited: (appId, pid, success, detail) => {
            shell.diag("app.exited", { id: appId, pid: pid, success: success, detail: detail })
            const started = shell._pending[appId]
            if (started !== undefined) {
                delete shell._pending[appId]
                if (!success)
                    shell.notify(shell.appName(appId) + " quit while starting (" + detail + ")", "error")
            }
        }
        onCountChanged: shell.diag("apps.catalog", { count: count })
    }

    // ── Platform adapters (Pocket.Platform) ──────────────────────────────────
    readonly property DisplayPower display: DisplayPower {
        onOnChanged: shell.diag("display.power", { on: on })
    }
    readonly property PowerKey powerKey: PowerKey {
        onActiveChanged: shell.diag("powerkey.grab", { active: active, error: error || "-" })
        onPressed: {
            shell.diag("powerkey.pressed", { display: shell.display.on ? "on" : "off" })
            shell.setDisplayOn(!shell.display.on)
        }
    }
    readonly property OutputConfig output: OutputConfig {
        onApplied: (ok, detail) => shell.diag("output.applied", { ok: ok, rotation: rotation, detail: JSON.stringify(detail) })
        onRotationChanged: shell.diag("output.rotation", { rotation: rotation })
    }
    readonly property Brightness brightness: Brightness {}
    readonly property OrientationSensor sensor: OrientationSensor {
        enabled: shell.rotationMode === "auto" && shell.display.on
        onOrientationChanged: shell.applyRotation()
    }
    readonly property OnScreenKeyboard osk: OnScreenKeyboard {
        onVisibleChanged: shell.diag("osk.visible", { visible: visible })
    }
    readonly property Audio audio: Audio {}

    // ── Injected by the Quickshell layer ─────────────────────────────────────
    property var windows: null     // services/WindowService.qml
    property var system: null      // services/SystemStatus.qml

    readonly property var windowList: windows ? windows.toplevels : []
    readonly property int windowCount: windowList.length

    // ── UI-owned navigation state ────────────────────────────────────────────
    property bool homeRequested: true
    // Home is shown on request, and always when no application window exists.
    readonly property bool homeVisible: homeRequested || windowCount === 0
    property string homePage: "home"   // "home" | "apps" | "switcher"
    property string panel: ""          // "" | "quicksettings" | "dev"
    property string rotationMode: "portrait" // "portrait" | "landscape" | "auto"
    readonly property bool landscapeOutput: output.rotation === 90 || output.rotation === 270

    function goHome() {
        panel = ""
        osk.setVisible(false) // the launcher's search field shows it again on focus
        homePage = "home"
        homeRequested = true
        diag("nav.home", { windows: windowCount })
    }
    function showPage(page) {
        panel = ""
        homePage = page
        homeRequested = true
        diag("nav.page", { page: page })
    }
    function togglePanel(name) {
        panel = panel === name ? "" : name
        diag("panel", { open: panel || "-" })
    }
    function closePanel() {
        if (panel !== "") {
            panel = ""
            diag("panel", { open: "-" })
        }
    }

    // ── Applications and windows ─────────────────────────────────────────────
    property var _pending: ({})   // appId -> launch time (ms), until a window or an exit

    function appName(appId) {
        const e = apps.get(appId)
        return e.name || appId
    }
    // Window of a catalog app, matched by Wayland app_id (ID or StartupWMClass).
    function windowFor(appId) {
        for (const w of windowList)
            if (w.appId === appId || apps.idForWindow(w.appId) === appId)
                return w
        return null
    }
    // Home/launcher tap: bring an existing window forward instead of starting a duplicate.
    function openApp(appId) {
        const w = windowFor(appId)
        if (w) {
            diag("app.reactivate", { id: appId })
            activateWindow(w)
            return
        }
        if (_pending[appId] !== undefined && Date.now() - _pending[appId] < 10000) {
            notify(appName(appId) + " is starting…", "info")
            return
        }
        diag("app.launch", { id: appId })
        _pending[appId] = Date.now()
        _pendingCheck.restart()
        notify("Starting " + appName(appId) + "…", "info")
        apps.launch(appId)
    }
    function activateWindow(w) {
        diag("window.activate", { appId: w.appId || "-" })
        w.activate()
        panel = ""
        homeRequested = false
    }
    function closeWindow(w) {
        diag("window.close", { appId: w.appId || "-" })
        w.close()
    }
    // Called by WindowService.
    function windowOpened(w) {
        const id = apps.idForWindow(w.appId) || w.appId
        diag("window.opened", { appId: w.appId || "-", app: id || "-", windows: windowCount })
        if (_pending[id] !== undefined) {
            delete _pending[id]
            dismissInfo()
        }
        panel = ""
        homeRequested = false
    }
    function windowClosed(appId) {
        diag("window.closed", { appId: appId || "-", windows: windowCount })
        Qt.callLater(() => {
            if (windowCount === 0) {
                // Nothing left to switch to or type into: plain home, keyboard down.
                if (homePage === "switcher")
                    homePage = "home"
                osk.setVisible(false)
            }
        })
    }

    readonly property Timer _pendingCheck: Timer {
        interval: 20000
        onTriggered: {
            const now = Date.now()
            for (const id in shell._pending) {
                if (now - shell._pending[id] >= interval - 50) {
                    delete shell._pending[id]
                    shell.notify(shell.appName(id) + " did not open a window", "error")
                    shell.diag("app.nowindow", { id: id })
                }
            }
            if (Object.keys(shell._pending).length > 0)
                restart()
        }
    }

    // ── Display, power key, rotation ─────────────────────────────────────────
    // Display power management only; this is not a lock screen (ADR-0005).
    function setDisplayOn(on) {
        if (!on) {
            panel = ""
            osk.setVisible(false)
        }
        display.setOn(on)
    }
    function setRotationMode(mode) {
        rotationMode = mode
        diag("rotation.mode", { mode: mode })
        applyRotation()
    }
    function applyRotation() {
        if (!output.available)
            return
        if (rotationMode === "portrait")
            output.setRotation(0)
        else if (rotationMode === "landscape")
            output.setRotation(90)
        else if (sensor.claimed) {
            // iio-sensor-proxy orientation → wl_output transform (counter-clockwise).
            const map = { "normal": 0, "left-up": 90, "right-up": 270 }
            const r = map[sensor.orientation]
            if (r !== undefined)
                output.setRotation(r)
        }
    }

    // ── Clock (minute resolution; one wakeup per minute) ─────────────────────
    property date now: new Date()
    readonly property Timer _clock: Timer {
        running: true
        repeat: true
        interval: 1000 * (60 - new Date().getSeconds())
        onTriggered: {
            shell.now = new Date()
            interval = 1000 * (60 - shell.now.getSeconds()) + 50
        }
    }

    // Apps listed first on the home screen when installed (by desktop ID); the rest of
    // the home grid is filled alphabetically. A fixed default for Stage 2 (no settings).
    readonly property var favoriteIds: ["foot", "gtk3-demo", "gtk3-widget-factory", "org.gnome.Console",
        "firefox", "org.gnome.TextEditor", "org.gnome.Settings"]

    // ── Toasts ───────────────────────────────────────────────────────────────
    readonly property ListModel toasts: ListModel {}
    property int _toastSerial: 0
    function notify(text, kind) {
        if (kind === "info")
            dismissInfo()
        toasts.append({ serial: ++_toastSerial, text: text, kind: kind || "info",
                        expires: Date.now() + (kind === "error" ? 6000 : 3500) })
        _toastTimer.restart()
        diag("toast", { kind: kind || "info", text: JSON.stringify(text) })
    }
    function dismissInfo() {
        for (let i = toasts.count - 1; i >= 0; --i)
            if (toasts.get(i).kind === "info")
                toasts.remove(i)
    }
    function dismissToast(serial) {
        for (let i = toasts.count - 1; i >= 0; --i)
            if (toasts.get(i).serial === serial)
                toasts.remove(i)
    }
    readonly property Timer _toastTimer: Timer {
        interval: 500
        repeat: true
        running: shell.toasts.count > 0
        onTriggered: {
            const now = Date.now()
            for (let i = shell.toasts.count - 1; i >= 0; --i)
                if (shell.toasts.get(i).expires <= now)
                    shell.toasts.remove(i)
        }
    }

    // ── Diagnostics ──────────────────────────────────────────────────────────
    // One machine-greppable line per event: "pocket.shell.diag: event=<name> k=v ..."
    readonly property LoggingCategory _diag: LoggingCategory {
        name: "pocket.shell.diag"
        defaultLogLevel: LoggingCategory.Info
    }
    property string lastEvent: "none yet"
    function diag(name, fields) {
        let line = "event=" + name
        for (const k in fields)
            line += " " + k + "=" + fields[k]
        console.info(_diag, line)
        lastEvent = line
    }
}
