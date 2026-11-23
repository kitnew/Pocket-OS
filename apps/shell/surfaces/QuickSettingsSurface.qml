import QtQuick
import Quickshell
import Quickshell.Wayland
import "../features/quicksettings"
import "../state"

// Quick Settings: overlay-layer modal panel over everything (incl. the status bar),
// mapped only while open. Ignores exclusive zones so its scrim covers the screen.
PanelWindow {
    anchors { top: true; bottom: true; left: true; right: true }
    exclusionMode: ExclusionMode.Ignore
    WlrLayershell.layer: WlrLayer.Overlay
    WlrLayershell.namespace: "pocket-quicksettings"
    WlrLayershell.keyboardFocus: WlrKeyboardFocus.None
    color: "transparent"
    visible: Shell.panel === "quicksettings"

    QuickSettings { anchors.fill: parent }
}
