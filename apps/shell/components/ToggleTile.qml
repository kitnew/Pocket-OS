import QtQuick
import "../theme"

// Quick Settings tile. States: on (checked), off, busy (a change is pending),
// unsupported (supported: false — shown dimmed with the reason in `detail`, not
// clickable). The tile never flips itself: `checked` must come from real state.
Rectangle {
    id: root

    property string icon
    property string label
    property string detail
    property bool checked: false
    property bool supported: true
    property bool busy: false
    signal activated()

    implicitWidth: 160
    implicitHeight: 60
    radius: Theme.radius
    color: !supported ? Theme.surface
         : tap.pressed ? Theme.surfacePressed
         : checked ? Theme.accent : Theme.surfaceRaised
    border.color: supported ? "transparent" : Theme.outline
    border.width: 1
    Behavior on color { ColorAnimation { duration: Theme.duration } }
    Accessible.role: Accessible.CheckBox
    Accessible.name: label
    Accessible.checked: checked

    Row {
        anchors.fill: parent
        anchors.leftMargin: Theme.space3
        anchors.rightMargin: Theme.space2
        spacing: Theme.space3
        Icon {
            anchors.verticalCenter: parent.verticalCenter
            name: root.icon
            opacity: root.supported ? 1 : 0.4
        }
        Column {
            anchors.verticalCenter: parent.verticalCenter
            width: parent.width - Theme.icon - Theme.space3
            spacing: 1
            Text {
                width: parent.width
                text: root.label
                color: root.supported ? (root.checked ? Theme.accentText : Theme.text) : Theme.textDisabled
                font.pixelSize: Theme.fontLabel
                font.weight: Font.DemiBold
                elide: Text.ElideRight
            }
            Text {
                width: parent.width
                visible: text.length > 0
                text: root.busy ? "…" : root.detail
                color: root.checked && root.supported ? Theme.accentText : Theme.textMuted
                opacity: root.checked ? 0.85 : 1
                font.pixelSize: Theme.fontCaption
                elide: Text.ElideRight
            }
        }
    }
    TapHandler {
        id: tap
        enabled: root.supported && !root.busy
        gesturePolicy: TapHandler.ReleaseWithinBounds
        onTapped: root.activated()
    }
}
