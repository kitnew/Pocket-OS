import QtQuick
import Quickshell
import Quickshell.Wayland
import "../state"

// Window management adapter (Quickshell-specific): application windows as reported by
// Phoc through wlr-foreign-toplevel-management-v1. Exposes plain properties to the
// portable shell: `toplevels` (objects with appId, title, activated, activate(),
// close()) and `active`. Phoc 0.59 cannot minimize (no such request handling), so the
// shell never relies on it (ADR-0005).
Scope {
    readonly property var toplevels: ToplevelManager.toplevels.values
    readonly property var active: ToplevelManager.activeToplevel
    readonly property bool canActivate: true
    readonly property bool canClose: true
    readonly property bool canMinimize: false

    Connections {
        target: ToplevelManager.toplevels
        // Inserted after the handle's first `done`, so appId/title are already set.
        function onObjectInsertedPost(object, index) { Shell.windowOpened(object) }
        function onObjectRemovedPost(object, index) { Shell.windowClosed(object ? object.appId : "") }
    }
}
