import QtQuick
import "../theme"

// Quick Settings tile. States: on (checked), off, busy (a change is pending),
// unsupported (supported: false — shown dimmed with the reason in `detail`, not
// clickable). The tile never flips itself: `checked` must come from real state.
// `momentary` tiles (an action, not a switch) never show the "on" color.
Pressable {
    id: root

    property string icon
    property string label
    property string detail
    property bool checked: false
    property bool supported: true
    property bool busy: false
    property bool momentary: false
    property bool dense: false      // 54 px rows (landscape Quick Settings)
    signal activated()

    readonly property bool on: checked && !momentary && supported
    implicitWidth: 160
    implicitHeight: dense ? 54 : 60
    interactive: supported && !busy
    pressScale: 0.97
    Accessible.role: momentary ? Accessible.Button : Accessible.CheckBox
    Accessible.name: label
    Accessible.checked: checked
    onTapped: activated()

    Rectangle {
        anchors.fill: parent
        radius: Theme.radius
        color: !root.supported ? Theme.surface
             : root.pressed ? Theme.surfacePressed
             : root.on ? Theme.accent : Theme.surfaceRaised
        border.color: root.supported ? "transparent" : Theme.outline
        border.width: 1
        Behavior on color { ColorAnimation { duration: Theme.duration } }
    }
    Row {
        anchors.fill: parent
        anchors.leftMargin: Theme.space3
        anchors.rightMargin: Theme.space2
        spacing: Theme.space3
        Rectangle {
            anchors.verticalCenter: parent.verticalCenter
            width: 34; height: 34; radius: 17
            color: root.on ? Qt.rgba(1, 1, 1, 0.18) : Theme.surfaceHigh
            Icon {
                anchors.centerIn: parent
                name: root.icon
                size: Theme.iconSmall + 2
                opacity: root.supported ? 1 : 0.4
            }
        }
        Column {
            anchors.verticalCenter: parent.verticalCenter
            width: parent.width - 34 - Theme.space3
            spacing: 1
            Text {
                width: parent.width
                text: root.label
                color: root.supported ? (root.on ? Theme.accentText : Theme.text) : Theme.textDisabled
                font.pixelSize: Theme.fontLabel
                font.weight: Font.DemiBold
                elide: Text.ElideRight
            }
            Text {
                width: parent.width
                visible: text.length > 0
                text: root.busy ? "…" : root.detail
                color: root.on ? Theme.accentText : Theme.textMuted
                opacity: root.on ? 0.85 : 1
                font.pixelSize: Theme.fontCaption
                elide: Text.ElideRight
            }
        }
    }
}
