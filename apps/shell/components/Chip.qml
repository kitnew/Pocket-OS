import QtQuick
import "../theme"

// A compact pill: optional status dot or icon, and text. Tappable when `interactive`.
Pressable {
    id: root
    property string text
    property string icon
    property color dotColor: "transparent"
    property color tint: Theme.surfaceRaised
    property color textColor: Theme.text
    property bool selected: false
    interactive: false
    implicitHeight: 32
    implicitWidth: row.implicitWidth + 2 * Theme.space3
    Accessible.role: interactive ? Accessible.Button : Accessible.StaticText
    Accessible.name: text

    Rectangle {
        anchors.fill: parent
        radius: height / 2
        color: root.pressed ? Theme.surfacePressed : root.selected ? Theme.accentSoft : root.tint
        border.width: root.selected ? 1 : 0
        border.color: Theme.accent
    }
    Row {
        id: row
        anchors.centerIn: parent
        spacing: Theme.space2
        Rectangle {
            anchors.verticalCenter: parent.verticalCenter
            visible: root.dotColor.a > 0
            width: 8; height: 8; radius: 4
            color: root.dotColor
        }
        Icon {
            anchors.verticalCenter: parent.verticalCenter
            visible: root.icon.length > 0
            name: root.icon
            size: Theme.iconSmall
        }
        Text {
            anchors.verticalCenter: parent.verticalCenter
            text: root.text
            visible: text.length > 0
            color: root.textColor
            font.pixelSize: Theme.fontLabel
            font.weight: Font.Medium
        }
    }
}
