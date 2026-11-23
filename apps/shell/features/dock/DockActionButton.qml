import QtQuick
import "../../theme"

// One desktop action. Shows what the computer reported, never a local guess:
//   available  → tappable
//   busy       → Core accepted the request, waiting for the computer's confirmation
//   result     → the last confirmed outcome ({ ok, text, time }) or nothing
//   unavailable → dimmed with the agent's reason (not tappable)
Rectangle {
    id: root

    property string label
    property string category
    property bool available: true
    property string reason
    property bool busy: false
    property var result: null
    signal activated()

    implicitHeight: 76
    radius: Theme.radius
    color: !available ? Theme.surface
         : tap.pressed ? Theme.surfacePressed
         : busy ? Theme.surfacePressed : Theme.surfaceRaised
    border.color: available ? (result && !result.ok ? Theme.danger : "transparent") : Theme.outline
    border.width: 1
    Behavior on color { ColorAnimation { duration: Theme.durationFast } }
    Accessible.role: Accessible.Button
    Accessible.name: label

    Column {
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.verticalCenter: parent.verticalCenter
        anchors.margins: Theme.space3
        spacing: 2
        Text {
            width: parent.width
            text: root.category.toUpperCase()
            visible: text.length > 0
            color: Theme.textMuted
            font.pixelSize: Theme.fontCaption
            font.letterSpacing: 0.5
            elide: Text.ElideRight
        }
        Text {
            width: parent.width
            text: root.label
            color: root.available ? Theme.text : Theme.textDisabled
            font.pixelSize: Theme.fontBody
            font.weight: Font.DemiBold
            elide: Text.ElideRight
        }
        Text {
            width: parent.width
            text: !root.available ? root.reason
                : root.busy ? "Waiting for the computer…"
                : root.result ? (root.result.ok ? "✓ " : "✕ ") + root.result.text + " · " + root.result.time
                : "Tap to run"
            color: !root.available || root.busy ? Theme.textMuted
                 : root.result ? (root.result.ok ? Theme.success : Theme.danger) : Theme.textMuted
            font.pixelSize: Theme.fontCaption
            elide: Text.ElideRight
        }
    }

    TapHandler {
        id: tap
        enabled: root.available && !root.busy
        gesturePolicy: TapHandler.ReleaseWithinBounds
        onTapped: root.activated()
    }
}
