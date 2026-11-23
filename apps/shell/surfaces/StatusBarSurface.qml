import QtQuick
import Quickshell
import Quickshell.Wayland
import "../features/statusbar"
import "../theme"

// Status bar: top-layer strip with an exclusive zone, so maximized apps are laid out
// below it. No keyboard focus; input only within its own area.
PanelWindow {
    anchors { top: true; left: true; right: true }
    implicitHeight: Theme.statusBarHeight
    exclusionMode: ExclusionMode.Auto
    WlrLayershell.layer: WlrLayer.Top
    WlrLayershell.namespace: "pocket-statusbar"
    WlrLayershell.keyboardFocus: WlrKeyboardFocus.None
    color: Theme.background

    StatusBar { anchors.fill: parent }
}
