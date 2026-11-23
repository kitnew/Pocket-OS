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
//   core-owned        core.mode, app catalog + launching, Dock link state (pocketd)
//   compositor-owned  windows, focus, output power/transform (Phoc; mirrored here)
//   system-owned      battery, network, bluetooth, backlight, sensors (their services)
//   UI-owned          homeVisible, homePage, panel, rotationMode, toasts, last Dock action
//                     results (this file)
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
    // Dock Mode is the core's mode, never a local flag (ADR-0007). It survives core
    // restarts within the session (the core restores it), and CoreClient keeps the last
    // mode while the core is away, so a restart does not flip the UI out and back.
    readonly property bool docked: core.mode === "dock"
    onDockedChanged: {
        diag("dock.mode", { docked: docked })
        // Dock Mode is landscape; the user's rotation preference is kept and restored.
        applyRotation()
        if (docked) {
            panel = ""
            homePage = "home"   // the Dock surface
            homeRequested = true
            dockNav.reset()     // entering Dock Mode starts at Ambient Home, unpinned
        }
        dockConfirm = null
    }

    function requestMode(mode) {
        diag("mode.request", { mode: mode })
        core.requestMode(mode)
    }

    // ── Dock session (pocketd owns the link; this is a mirror plus dispatch) ──
    readonly property DockClient dock: DockClient {
        core: shell.core
        onChanged: shell._dockChanged()
        onActionsChanged: shell.diag("dock.actions", { ids: actions.map(a => a.id + (a.available ? "" : "(off)")).join(",") || "-" })
        onRequestFailed: (op, error) => {
            shell.diag("dock.request.failed", { op: op, error: JSON.stringify(error) })
            shell.notify("Dock " + op + " failed: " + error.replace(/^[A-Za-z]+: /, ""), "error")
        }
        onActionFinished: (request, action, ok, code, message, data) => shell._dockFinished(request, action, ok, code, message, data)
        onActionProgress: (request, action, step, total, label) => shell._dockProgress(request, action, step, total, label)
        onDeckChanged: {
            const nav = deck.navigation || {}
            if (nav.idle_timeout_s !== undefined)
                shell.dockNav.setIdleTimeout(nav.idle_timeout_s * 1000)
            shell.diag("dock.deck", { source: deck.source || "-", pages: (deck.pages || []).length,
                                      idle_s: nav.idle_timeout_s, error: JSON.stringify(deck.error || "") })
            if (deck.error)
                shell.notify("Deck configuration ignored: " + deck.error, "error")
        }
        onLiveChanged: shell.diag("dock.live", { topics: Object.keys(live).map(t => t + (live[t].available ? "" : "(off)") + (live[t].stale ? "(stale)" : "")).join(",") || "-" })
        onNotification: (n) => shell._dockNotification(n)
        onNotificationClosed: (nid) => shell._dockNotificationClosed(nid)
    }
    // Never leave a confirmation open for a link that is gone (UX spec, navigation rule 8).
    readonly property bool _dockLinkUp: dock.state === "connected"
    on_DockLinkUpChanged: if (!_dockLinkUp && dockConfirm !== null) {
        notify("Connection lost: " + dockConfirm.label + " was not sent", "error")
        cancelDockConfirm()
    }

    // ── Dock navigation and Control Deck (UI-owned; docs/dock-mode/ux-specification.md) ──
    readonly property DockNav dockNav: DockNav {
        onChanged: (what) => shell.diag("dock.nav", { view: view, page: page || "-", pinned: pinned, why: what })
    }
    // The advertised action for an id, or null.
    function dockAction(id) {
        const list = dock.actions
        for (let i = 0; i < list.length; ++i)
            if (list[i].id === id)
                return list[i]
        return null
    }
    // Tile results, by tile id: { ok, code, text, time, request, uncertain, progress }.
    property var tileResults: ({})
    property var _tileByAction: ({})  // action id -> tile id that is waiting for it
    // Open confirmation: { tile, retry (request id or 0), label, text } or null.
    property var dockConfirm: null

    function _setTileResult(tileId, r) {
        const all = Object.assign({}, tileResults)
        if (r === null)
            delete all[tileId]
        else
            all[tileId] = r
        tileResults = all
    }
    // A tap on a deck tile.
    function runDeckTile(tile) {
        const a = dockAction(tile.action)
        diag("dock.tile", { tile: tile.id, action: tile.action, advertised: a !== null })
        if (!a || !a.available || dock.pendingActions[tile.action] !== undefined)
            return
        if (a.confirm) {
            dockConfirm = { tile: tile, retry: 0, label: tile.label,
                            text: "Run “" + tile.label + "” on " + (dock.hostName || "the computer") + "?" }
            dockNav.openModal("confirm")
            return
        }
        _sendTile(tile, 0)
    }
    // "Outcome unknown" → Retry with the same request key (Core never retries by itself).
    function retryDeckTile(tile) {
        const r = tileResults[tile.id]
        const a = dockAction(tile.action)
        if (!r || !r.uncertain || !a || !a.available)
            return
        if (!a.idempotent || a.confirm) {
            dockConfirm = { tile: tile, retry: r.request, label: tile.label,
                            text: "“" + tile.label + "” may already have run on " + (dock.hostName || "the computer")
                                  + ". Send it again? If it did run, the computer will not run it twice." }
            dockNav.openModal("confirm")
            return
        }
        _sendTile(tile, r.request)
    }
    function acceptDockConfirm() {
        const c = dockConfirm
        dockConfirm = null
        dockNav.closeModal()
        if (c)
            _sendTile(c.tile, c.retry)
    }
    function cancelDockConfirm() {
        dockConfirm = null
        dockNav.closeModal()
    }
    function _sendTile(tile, retryRequest) {
        const tiles = Object.assign({}, _tileByAction)
        tiles[tile.action] = tile.id
        _tileByAction = tiles
        _setTileResult(tile.id, null)
        if (retryRequest > 0) {
            diag("dock.retry", { tile: tile.id, request: retryRequest })
            dock.retry(retryRequest, tile.action)
        } else {
            dock.execute(tile.action, tile.params || {})
        }
    }
    function _dockProgress(request, action, step, total, label) {
        const tileId = _tileByAction[action]
        diag("dock.action.progress", { request: request, id: action, step: step, total: total })
        if (tileId)
            _setTileResult(tileId, { ok: true, progress: true, text: "Step " + step + "/" + total + " · " + label,
                                     time: "", request: request, uncertain: false })
    }

    // ── Laptop notifications (Stage 4.4) ──
    // History: newest first, memory only (max 50). Banner: the latest non-silent one,
    // shown only in Dock Mode. Content is never written to the diagnostic log.
    readonly property ListModel dockNotifications: ListModel {}
    property var dockBanner: null
    function _dockNotification(n) {
        diag("dock.notification", { nid: n.nid, silent: !!n.silent, redacted: !!n.redacted })
        dockNotifications.insert(0, { nid: n.nid, app: n.app || "", summary: n.summary || "", body: n.body || "",
                                      redacted: !!n.redacted, urgency: n.urgency || 1,
                                      time: Qt.formatTime(new Date(), "HH:mm") })
        while (dockNotifications.count > 50)
            dockNotifications.remove(dockNotifications.count - 1)
        if (!n.silent && docked) {
            dockBanner = n
            if (n.urgency === 2)
                _bannerTimer.stop()   // critical: stays until tapped
            else
                _bannerTimer.restart()
        }
    }
    function _dockNotificationClosed(nid) {
        for (let i = dockNotifications.count - 1; i >= 0; --i)
            if (dockNotifications.get(i).nid === nid)
                dockNotifications.remove(i)
        if (dockBanner && dockBanner.nid === nid)
            dismissDockBanner()
    }
    function dismissDockBanner() {
        dockBanner = null
        _bannerTimer.stop()
    }
    function clearDockNotifications() {
        dockNotifications.clear()
        dismissDockBanner()
    }
    readonly property Timer _bannerTimer: Timer {
        interval: 6000
        onTriggered: shell.dockBanner = null
    }

    property string _dockLogged: ""
    function _dockChanged() {
        const line = [dock.supported, dock.state, dock.hostName, dock.errorCode, dock.retryIn, dock.connectionWanted].join()
        if (line === _dockLogged)
            return
        _dockLogged = line
        diag("dock.state", { supported: dock.supported, state: dock.state || "-", host: JSON.stringify(dock.hostName),
                             error: dock.errorCode || "-", retry_ms: dock.retryRemainingMs(), wanted: dock.connectionWanted })
    }
    // The connection policy is Core's (ADR-0007, Stage 3.1): entering Dock Mode connects,
    // Disconnect is remembered for the session, and a restarted core reconnects only if
    // the link was wanted. The shell never connects on its own, so no UI state change
    // (a shell or core restart, the Dock object reappearing) can override a Disconnect.
    function connectDock() {
        diag("dock.connect", { state: dock.state })
        dock.connectHost()
    }
    function disconnectDock() {
        diag("dock.disconnect", { state: dock.state })
        dock.disconnectHost()
    }
    // Parameters the UI sends per action (the agent has defaults for everything).
    function dockParams(action) {
        if (action === "desktop.notify")
            return { title: "Pocket OS", body: "Sent from Pocket Dock at " + Qt.formatTime(new Date(), "HH:mm:ss") }
        return {}
    }
    function runDockAction(action) {
        diag("dock.action", { id: action })
        dock.execute(action, dockParams(action))
    }
    // Last outcome per action id: { ok, text, time }. UI-owned, reset by a shell restart.
    property var dockResults: ({})
    function _dockFinished(request, action, ok, code, message, data) {
        const uncertain = !ok && (code === "timeout" || code === "connection_lost")
        let text = ok ? "Done on " + (dock.hostName || "computer") : (message || code)
        if (ok && message === "duplicate")
            text = "Already done on " + (dock.hostName || "computer")
        else if (uncertain)
            text = "Outcome unknown"
        else if (!ok && code === "core_lost")
            text = "Pocket Core restarted; check the computer"
        if (ok && action === "desktop.notify")
            text = "Notification shown on " + (dock.hostName || "computer")
        else if (ok && action === "media.play_pause") {
            try { text = "Toggled " + JSON.parse(data).player } catch (e) {}
        }
        const r = Object.assign({}, dockResults)
        r[action] = { ok: ok, code: code, text: text, time: Qt.formatTime(new Date(), "HH:mm:ss") }
        dockResults = r
        const tileId = _tileByAction[action]
        if (tileId !== undefined) {
            const tiles = Object.assign({}, _tileByAction)
            delete tiles[action]
            _tileByAction = tiles
            _setTileResult(tileId, { ok: ok, code: code, text: text, request: request, uncertain: uncertain,
                                     progress: false, time: Qt.formatTime(new Date(), "HH:mm") })
        }
        diag("dock.action.finished", { request: request, id: action, ok: ok, code: code || "-",
                                       message: JSON.stringify(message), data: data || "-" })
        // Tiles show their own result; only untargeted requests (IPC, Link view) toast.
        if (!ok && tileId === undefined)
            notify(dock.hostName ? action + " on " + dock.hostName + ": " + text : text, "error")
    }

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
        // A shell (re)started while docked: put the output back to landscape once the
        // output manager is bound.
        onAvailableChanged: if (available && shell.docked) shell.applyRotation()
    }
    readonly property Brightness brightness: Brightness {}
    readonly property OrientationSensor sensor: OrientationSensor {
        enabled: shell.rotationMode === "auto" && shell.display.on
        onOrientationChanged: shell.applyRotation()
    }
    readonly property OnScreenKeyboard osk: OnScreenKeyboard {
        onVisibleChanged: shell.diag("osk.visible", { visible: visible })
    }
    // Sound cards registered by the kernel: tells "no hardware" from "no audio service".
    readonly property Audio soundCards: Audio {}

    // ── Injected by the Quickshell layer ─────────────────────────────────────
    property var windows: null     // services/WindowService.qml
    property var system: null      // services/SystemStatus.qml
    property var audio: null       // services/AudioService.qml (PipeWire default output)

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
        if (docked)
            dockNav.navigate("home")
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

    // ── Audio (PipeWire owns the state; `audio` is the injected adapter) ─────
    function setVolume(v) { return audio !== null && audio.setVolume(v) }
    function setMuted(m) { return audio !== null && audio.setMuted(m) }
    readonly property string volumeStatus: {
        if (soundCards.cards === 0)
            return "No sound card detected"
        if (!audio || !audio.connected)
            return "Audio service not running"
        if (!audio.available)
            return "No audio output"
        return audio.outputName + (audio.muted ? " · muted" : " · " + Math.round(audio.volume * 100) + "%")
    }
    // One diagnostic line per settled change, whatever its source (slider, IPC, wpctl,
    // device switch). The timer runs only after a change; nothing polls.
    readonly property string _audioState: audio ? [audio.available, audio.outputId, Math.round(audio.volume * 100), audio.muted].join() : ""
    on_AudioStateChanged: _audioLog.restart()
    readonly property Timer _audioLog: Timer {
        interval: 400
        onTriggered: shell.diag("audio.state", shell.audio
            ? { available: shell.audio.available, output: "\"" + shell.audio.outputName + "\"", id: shell.audio.outputId,
                volume: Math.round(shell.audio.volume * 100), muted: shell.audio.muted }
            : { available: false })
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
        if (docked) {
            // Either landscape orientation is fine while docked; never portrait.
            if (output.rotation !== 90 && output.rotation !== 270)
                output.setRotation(90)
            return
        }
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
