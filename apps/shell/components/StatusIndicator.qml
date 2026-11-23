import QtQuick
import "../theme"

// Status bar item: an icon with optional text. `dim` for "known but inactive".
Row {
    property string icon
    property string text
    property bool dim: false
    spacing: Theme.space1
    opacity: dim ? 0.45 : 1
    Icon {
        anchors.verticalCenter: parent.verticalCenter
        name: parent.icon
        size: Theme.iconSmall
        visible: parent.icon.length > 0
    }
    Text {
        anchors.verticalCenter: parent.verticalCenter
        visible: text.length > 0
        text: parent.text
        color: Theme.text
        font.pixelSize: Theme.fontLabel
    }
}
