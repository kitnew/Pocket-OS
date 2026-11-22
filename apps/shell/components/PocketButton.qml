import QtQuick
import "../theme"

// Touch-first button. Reusable across handheld and dock layouts; it knows nothing
// about what it triggers — callers handle `activated`.
Rectangle {
    id: root

    property alias text: label.text
    property bool checked: false
    signal activated(var point)

    implicitWidth: Math.max(Theme.touchTarget * 4, label.implicitWidth + 2 * Theme.spacing)
    implicitHeight: Theme.touchTarget * 1.5
    radius: Theme.radius
    color: tap.pressed ? Theme.surfaceActive : (checked ? Theme.accent : Theme.surface)
    scale: tap.pressed ? 0.96 : 1.0
    opacity: enabled ? 1.0 : 0.4   // disabled items receive no pointer events

    Behavior on color { ColorAnimation { duration: 90 } }
    Behavior on scale { NumberAnimation { duration: 90 } }

    Text {
        id: label
        anchors.centerIn: parent
        color: Theme.text
        font.pixelSize: Theme.fontBody + 4
    }

    TapHandler {
        id: tap
        onTapped: (eventPoint) => root.activated(eventPoint)
    }
}
