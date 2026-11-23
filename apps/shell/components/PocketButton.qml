import QtQuick
import "../theme"

// Text button. `primary` uses the accent color. Reusable across layouts; it knows
// nothing about what it triggers: callers handle `activated(point)`.
Rectangle {
    id: root

    property alias text: label.text
    property bool checked: false   // Stage 1 name; same look as primary
    property bool primary: checked
    signal activated(var point)

    implicitWidth: Math.max(Theme.touchTarget * 2.5, label.implicitWidth + 2 * Theme.space5)
    implicitHeight: Theme.touchTarget
    radius: height / 2
    color: tap.pressed ? Theme.surfacePressed : (primary ? Theme.accent : Theme.surfaceRaised)
    scale: tap.pressed ? 0.97 : 1.0
    opacity: enabled ? 1.0 : 0.4   // disabled items receive no pointer events

    Behavior on color { ColorAnimation { duration: Theme.durationFast } }
    Behavior on scale { NumberAnimation { duration: Theme.durationFast } }

    Text {
        id: label
        anchors.centerIn: parent
        color: root.primary ? Theme.accentText : Theme.text
        font.pixelSize: Theme.fontBody
        font.weight: Font.Medium
    }

    TapHandler {
        id: tap
        gesturePolicy: TapHandler.ReleaseWithinBounds
        onTapped: (eventPoint) => root.activated(eventPoint)
    }
}
