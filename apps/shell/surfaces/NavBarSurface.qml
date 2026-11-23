import QtQuick
import Quickshell
import Quickshell.Wayland
import "../features/navbar"
import "../state"
import "../theme"

// Navigation bar: bottom strip with an exclusive zone (apps are laid out above it; the
// on-screen keyboard stacks above it too).
//
// Overlay layer, not Top: Phoc applies exclusive zones layer by layer (overlay first)
// and, within a layer, oldest surface first. squeekboard (Top layer) keeps its surface
// once created, so with both on Top a shell restart put the keyboard below the bar.
// On Overlay the bar always owns the bottom edge, and it also stays reachable above
// fullscreen applications (there are no edge gestures to leave them).
// Unmapped on the immersive Dock (Shell.immersive): the Dock rail and its Controls sheet
// replace it there, and it returns whenever an app, the launcher or the switcher shows.
PanelWindow {
    visible: !Shell.immersive
    anchors { bottom: true; left: true; right: true }
    implicitHeight: Theme.navBarHeight
    exclusionMode: ExclusionMode.Auto
    WlrLayershell.layer: WlrLayer.Overlay
    WlrLayershell.namespace: "pocket-navbar"
    WlrLayershell.keyboardFocus: WlrKeyboardFocus.None
    color: Theme.background

    NavBar { anchors.fill: parent }
}
