import QtQuick
import Quickshell
import Quickshell.Wayland
import "../features/statusbar"
import "../state"
import "../theme"

// Status bar: top-layer strip with an exclusive zone, so maximized apps are laid out
// below it. No keyboard focus; input only within its own area.
// While the on-screen keyboard is shown in landscape the bar is unmapped: the 360 px
// landscape height is too short to spend 28 px on status while typing. The navigation
// bar (Home, keyboard dismiss) stays; the status bar returns when the keyboard hides.
// The immersive Dock (Shell.immersive) draws its own status, so the bar is unmapped there.
PanelWindow {
    visible: !(Shell.landscapeOutput && Shell.osk.visible) && !Shell.immersive
    anchors { top: true; left: true; right: true }
    implicitHeight: Theme.statusBarHeight
    exclusionMode: ExclusionMode.Auto
    WlrLayershell.layer: WlrLayer.Top
    WlrLayershell.namespace: "pocket-statusbar"
    WlrLayershell.keyboardFocus: WlrKeyboardFocus.None
    color: Theme.background

    StatusBar { anchors.fill: parent }
}
