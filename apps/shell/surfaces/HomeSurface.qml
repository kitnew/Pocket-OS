import QtQuick
import Quickshell
import Quickshell.Wayland
import "../features/devpanel"

// Full-screen wlr-layer-shell surface hosting the Stage 1 development panel.
// This file (and shell.qml) are the only Quickshell-specific parts of the shell.
PanelWindow {
    anchors { top: true; bottom: true; left: true; right: true }
    exclusionMode: ExclusionMode.Ignore
    WlrLayershell.layer: WlrLayer.Bottom
    WlrLayershell.namespace: "pocket-home"
    color: "transparent"

    DevPanel {
        anchors.fill: parent
        runtimeName: "Quickshell"
    }
}
