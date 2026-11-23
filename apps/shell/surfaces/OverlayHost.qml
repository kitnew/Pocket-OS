import QtQuick
import Quickshell
import Quickshell.Wayland
import "../features/toasts"
import "../state"
import "../theme"

// Transient overlays (toasts) above everything, near the bottom. The input region is
// limited to the toasts themselves, so the rest of the screen stays usable.
PanelWindow {
    anchors { bottom: true; left: true; right: true }
    margins.bottom: Shell.immersive ? Theme.space3 : Theme.navBarHeight + Theme.space2
    implicitHeight: Math.max(1, stack.implicitHeight)
    exclusionMode: ExclusionMode.Ignore
    WlrLayershell.layer: WlrLayer.Overlay
    WlrLayershell.namespace: "pocket-overlay"
    WlrLayershell.keyboardFocus: WlrKeyboardFocus.None
    color: "transparent"
    visible: Shell.toasts.count > 0
    mask: Region { item: stack }

    ToastStack {
        id: stack
        width: Math.min(parent.width - 2 * Theme.space3, 460)
        x: (parent.width - width) / 2
    }
}
