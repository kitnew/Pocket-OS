import QtQuick
import Quickshell
import Quickshell.Wayland
import "../features/navbar"
import "../theme"

// Navigation bar: bottom top-layer strip with an exclusive zone (apps are laid out above
// it; the on-screen keyboard stacks above it too).
PanelWindow {
    anchors { bottom: true; left: true; right: true }
    implicitHeight: Theme.navBarHeight
    exclusionMode: ExclusionMode.Auto
    WlrLayershell.layer: WlrLayer.Top
    WlrLayershell.namespace: "pocket-navbar"
    WlrLayershell.keyboardFocus: WlrKeyboardFocus.None
    color: Theme.background

    NavBar { anchors.fill: parent }
}
