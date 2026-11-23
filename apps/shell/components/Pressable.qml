import QtQuick
import "../theme"

// Base for every tappable surface: immediate press feedback (scale + tint within one
// frame), tap on release inside, optional long press. Children are laid out by the
// caller. `interactive: false` keeps it visible but inert (no feedback, no signals).
Item {
    id: root
    property bool interactive: true
    property bool longPressEnabled: false
    property real pressScale: 0.96
    readonly property bool pressed: tap.pressed && interactive
    signal tapped(var point)
    signal longPressed()

    scale: pressed ? pressScale : 1
    Behavior on scale { NumberAnimation { duration: Theme.durationFast; easing.type: Theme.easing } }

    TapHandler {
        id: tap
        enabled: root.interactive
        gesturePolicy: TapHandler.ReleaseWithinBounds
        longPressThreshold: root.longPressEnabled ? 0.5 : 0  // 0 = no long press: tap only
        onTapped: (eventPoint) => root.tapped(eventPoint)
        onLongPressed: if (root.longPressEnabled) root.longPressed()
    }
}
