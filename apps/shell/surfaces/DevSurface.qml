import QtQuick
import Quickshell
import Quickshell.Wayland
import "../features/devpanel"
import "../state"

// Developer diagnostics: overlay layer, between the bars, mapped only while open.
PanelWindow {
    anchors { top: true; bottom: true; left: true; right: true }
    exclusionMode: ExclusionMode.Normal
    WlrLayershell.layer: WlrLayer.Overlay
    WlrLayershell.namespace: "pocket-dev"
    WlrLayershell.keyboardFocus: WlrKeyboardFocus.None
    color: "transparent"
    visible: Shell.panel === "dev"

    DevPanel {
        anchors.fill: parent
        onCloseRequested: Shell.closePanel()
    }
}
