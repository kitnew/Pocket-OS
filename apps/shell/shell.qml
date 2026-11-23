// Pocket Shell entry point for the Quickshell runtime:
//   quickshell -p apps/shell/shell.qml
//
// Quickshell-specific code lives only here, in surfaces/ (layer-shell windows) and in
// services/ (Quickshell service adapters). Everything under state/, features/,
// components/ and theme/ is plain Qt Quick plus the Pocket.Core / Pocket.Platform modules.
import QtQuick
import Quickshell
import Quickshell.Io
import "services"
import "state"
import "surfaces"

ShellRoot {
    WindowService { id: windowService }
    SystemStatus { id: systemStatus }
    AudioService { id: audioService }

    Component.onCompleted: {
        Shell.windows = windowService
        Shell.system = systemStatus
        Shell.audio = audioService
    }

    // Developer/test control of the UI over Quickshell IPC, e.g.
    //   pocket-ctl ui open foot      pocket-ctl ui state
    // Every action goes through the same Shell functions the touch UI uses.
    IpcHandler {
        target: "pocket"
        function home(): void { Shell.goHome() }
        function page(name: string): void { Shell.showPage(name) }
        function panel(name: string): void { Shell.panel = name === "none" ? "" : name }
        function open(appId: string): void { Shell.openApp(appId) }
        function activate(appId: string): string {
            const w = Shell.windowFor(appId)
            if (!w) return "no window"
            Shell.activateWindow(w)
            return "ok"
        }
        function close(appId: string): string {
            const w = Shell.windowFor(appId)
            if (!w) return "no window"
            Shell.closeWindow(w)
            return "ok"
        }
        function rotation(mode: string): void { Shell.setRotationMode(mode) }
        function display(on: bool): void { Shell.setDisplayOn(on) }
        function brightness(percent: int): void { Shell.brightness.setValue(percent / 100) }
        function volume(percent: int): string { return Shell.setVolume(percent / 100) ? "ok" : "unavailable" }
        function mute(on: bool): string { return Shell.setMuted(on) ? "ok" : "unavailable" }
        // Dock: the same paths as the Quick Settings tile and the Dock page buttons.
        function mode(name: string): void { Shell.requestMode(name) }
        function dock(op: string): string {
            if (op === "connect") Shell.connectDock()
            else if (op === "disconnect") Shell.disconnectDock()
            else return "unknown op"
            return "ok"
        }
        function dockAction(id: string): string {
            const a = Shell.dock.actions.find(x => x.id === id)
            if (!a) return "not offered"
            if (!a.available) return "unavailable: " + a.reason
            if (Shell.dock.pendingActions[id] !== undefined) return "busy"
            Shell.runDockAction(id)
            return "ok"
        }
        function state(): string {
            return JSON.stringify({
                core: Shell.core.statusText, mode: Shell.core.mode, apps: Shell.apps.count,
                homeVisible: Shell.homeVisible, homePage: Shell.homePage, panel: Shell.panel,
                windows: Shell.windowList.map(w => ({ appId: w.appId, title: w.title, activated: w.activated })),
                rotationMode: Shell.rotationMode, rotation: Shell.output.rotation,
                display: Shell.display.on, powerKey: Shell.powerKey.active,
                brightness: Math.round(Shell.brightness.value * 100),
                sensor: { available: Shell.sensor.available, claimed: Shell.sensor.claimed, orientation: Shell.sensor.orientation },
                osk: { available: Shell.osk.available, visible: Shell.osk.visible },
                battery: Shell.system ? { available: Shell.system.batteryAvailable, percent: Math.round(Shell.system.batteryPercent), charging: Shell.system.batteryCharging } : null,
                wifi: Shell.system ? { available: Shell.system.wifiAvailable, enabled: Shell.system.wifiEnabled, connected: Shell.system.wifiConnected } : null,
                bluetooth: Shell.system ? { available: Shell.system.bluetoothAvailable, enabled: Shell.system.bluetoothEnabled } : null,
                audio: Shell.audio ? { connected: Shell.audio.connected, available: Shell.audio.available,
                                       output: Shell.audio.outputName, outputId: Shell.audio.outputId,
                                       volume: Math.round(Shell.audio.volume * 100), muted: Shell.audio.muted,
                                       cards: Shell.soundCards.cards } : null,
                dock: { docked: Shell.docked, supported: Shell.dock.supported, state: Shell.dock.state,
                        configured: Shell.dock.configured, host: Shell.dock.hostName, hostId: Shell.dock.hostId,
                        error: Shell.dock.errorCode, retryIn: Shell.dock.retryIn,
                        retryMs: Shell.dock.retryRemainingMs(), retryShown: Shell.dock.retrySeconds,
                        countdown: Shell.dock.countdownActive, wanted: Shell.dock.connectionWanted,
                        actions: Shell.dock.actions.map(a => ({ id: a.id, available: a.available })),
                        pending: Object.keys(Shell.dock.pendingActions), results: Shell.dockResults },
                toasts: Shell.toasts.count
            })
        }
    }

    // Stacking (bottom → top): apps | Top: home, status bar, nav bar, OSK | Overlay:
    // dev panel, quick settings, toasts.
    HomeSurface {}
    StatusBarSurface {}
    NavBarSurface {}
    DevSurface {}
    QuickSettingsSurface {}
    OverlayHost {}
}
