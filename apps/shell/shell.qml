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
        // Stage 4 Dock UI (the same Shell functions the rail, tiles and sheets call).
        //   nav home|deck|link | nav pin|unpin | nav page:ID | nav notifications|close|touch
        function nav(cmd: string): string {
            const op = cmd.split(":")[0]
            const arg = cmd.slice(op.length + 1)
            if (["home", "deck", "link"].includes(op)) Shell.dockNav.navigate(op)
            else if (op === "pin") Shell.dockNav.setPinned(true)
            else if (op === "unpin") Shell.dockNav.setPinned(false)
            else if (op === "page") Shell.dockNav.selectPage(arg)
            else if (op === "notifications") Shell.dockNav.openModal("notifications")
            else if (op === "close") { Shell.cancelDockConfirm(); if (Shell.dockDetail) Shell.closeTileDetail(); Shell.dockNav.closeModal() }
            else if (op === "touch") Shell.dockNav.interact()
            else if (op === "edit") Shell.beginDeckEdit()
            else if (op === "save") Shell.saveDeckEdit()
            else if (op === "cancel") Shell.cancelDeckEdit()
            else if (op === "addtile") Shell.addDeckTile(arg || Shell.dockNav.page)
            else if (op === "editpage") Shell.editDeckPage(arg || Shell.dockNav.page)
            else if (op === "closeeditor") Shell.closeDeckEditor()
            else if (op === "access") Shell.requestAccess(arg)
            else return "unknown op"
            return "ok"
        }
        // Tile details (long press) / editor (edit mode) for a tile id ("page/index").
        function tileinfo(id: string): string {
            for (const p of (Shell.shownDeck.pages || []))
                for (const t of p.tiles)
                    if (t.id === id) {
                        if (Shell.deckEditing) Shell.editDeckTile(t)
                        else Shell.openTileDetail(t)
                        return JSON.stringify(Shell.tileState(t, false))
                    }
            return "no such tile"
        }
        // Taps a deck tile by its id ("page/index"), e.g. tile general/3.
        function tile(id: string): string {
            for (const p of (Shell.dock.deck.pages || []))
                for (const t of p.tiles)
                    if (t.id === id) {
                        const r = Shell.tileResults[t.id]
                        if (r && r.uncertain) Shell.retryDeckTile(t)
                        else Shell.runDeckTile(t)
                        return Shell.dockConfirm ? "confirm" : "ok"
                    }
            return "no such tile"
        }
        function confirm(accept: bool): string {
            if (!Shell.dockConfirm) return "nothing to confirm"
            if (accept) Shell.acceptDockConfirm()
            else Shell.cancelDockConfirm()
            return "ok"
        }
        function dnd(on: bool): void { Shell.dock.setPreference("dnd", on) }
        function motion(reduce: bool): void { Shell.dock.setPreference("reduce_motion", reduce) }
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
                        pending: Object.keys(Shell.dock.pendingActions), results: Shell.dockResults,
                        platform: Shell.dock.platform,
                        nav: { view: Shell.dockNav.view, page: Shell.dockNav.page, pinned: Shell.dockNav.pinned,
                               modal: Shell.dockNav.modal, idleMs: Shell.dockNav.idleTimeoutMs,
                               dueInMs: Shell.dockNav.deadline < 0 ? -1 : Math.max(0, Math.round(Shell.dockNav.deadline - Date.now())) },
                        deck: { source: Shell.dock.deck.source || "", error: Shell.dock.deck.error || "",
                                pages: (Shell.dock.deck.pages || []).map(p => p.id) },
                        tiles: Shell.tileResults, confirm: Shell.dockConfirm ? Shell.dockConfirm.label : "",
                        tileKinds: (Shell.shownDeck.pages || []).reduce((o, p) => {
                            for (const t of p.tiles) o[t.id] = Shell.tileState(t, false).kind
                            return o
                        }, {}),
                        immersive: Shell.immersive, reduceMotion: Shell.reduceMotion,
                        editing: Shell.deckEditing, editTarget: Shell.deckEditTarget,
                        detail: Shell.dockDetail ? Shell.dockDetail.tile.id : "",
                        deckEditable: Shell.dock.deckEditable,
                        live: Object.keys(Shell.dock.live).reduce((o, t) => {
                            const v = Shell.dock.live[t]
                            o[t] = { available: v.available, stale: v.stale, rev: v.rev, data: v.data }
                            return o
                        }, {}),
                        prefs: Shell.dock.preferences,
                        notifications: Shell.dockNotifications.count, banner: Shell.dockBanner ? Shell.dockBanner.nid : "",
                        privileges: Shell.dock.actions.map(a => a.id + ":" + (a.privilege || "basic") + (a.available ? "" : "(off)")) },
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
