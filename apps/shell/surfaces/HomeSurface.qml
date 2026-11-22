import QtQuick
import Quickshell
import Quickshell.Wayland
import "../features/stage0"

// Full-screen wlr-layer-shell surface hosting the Stage 0 panel.
// This file (and shell.qml) are the only Quickshell-specific parts of the shell.
PanelWindow {
    anchors { top: true; bottom: true; left: true; right: true }
    exclusionMode: ExclusionMode.Ignore
    WlrLayershell.layer: WlrLayer.Bottom
    WlrLayershell.namespace: "pocket-home"
    color: "transparent"

    Stage0Panel {
        anchors.fill: parent
        runtimeName: "Quickshell"
    }
}
