import QtQuick
import "../theme"

// Round icon-only button with a touch target of at least Theme.touchTarget.
// `flat` (default) shows a background only while pressed or checked.
Item {
    id: root

    property string icon
    property int iconSize: Theme.icon
    property bool checked: false
    property bool flat: true
    property string accessibleName: icon
    signal activated()

    implicitWidth: Theme.touchTarget
    implicitHeight: Theme.touchTarget
    opacity: enabled ? 1.0 : 0.35
    Accessible.role: Accessible.Button
    Accessible.name: accessibleName

    Rectangle {
        anchors.centerIn: parent
        width: Math.min(parent.width, parent.height)
        height: width
        radius: width / 2
        color: tap.pressed ? Theme.surfacePressed
             : root.checked ? Theme.accent
             : root.flat ? "transparent" : Theme.surfaceRaised
        Behavior on color { ColorAnimation { duration: Theme.durationFast } }
    }
    Icon {
        anchors.centerIn: parent
        name: root.icon
        size: root.iconSize
    }
    TapHandler {
        id: tap
        gesturePolicy: TapHandler.ReleaseWithinBounds
        onTapped: root.activated()
    }
}
