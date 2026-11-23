import QtQuick
import Quickshell
import Quickshell.Wayland
import "../features/home"
import "../state"
import "../theme"

// Home (home page, launcher, window switcher). A top-layer surface between the status
// and navigation bars, mapped only while Shell.homeVisible: it covers application
// windows when shown and does not exist for input when hidden. Phoc cannot minimize
// windows, so "go home" means showing this surface above them (ADR-0005).
// Keyboard focus on demand: only when the search field is used.
PanelWindow {
    anchors { top: true; bottom: true; left: true; right: true }
    exclusionMode: ExclusionMode.Normal
    WlrLayershell.layer: WlrLayer.Top
    WlrLayershell.namespace: "pocket-home"
    WlrLayershell.keyboardFocus: WlrKeyboardFocus.OnDemand
    color: Theme.background
    visible: Shell.homeVisible

    HomeView { anchors.fill: parent }
}
