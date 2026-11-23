import QtQuick
import "../theme"

// Battery level drawn from real values (percent 0..100, charging). Not shown when
// `available` is false.
Row {
    id: root
    property bool available: false
    property real percent: 0
    property bool charging: false
    visible: available
    spacing: Theme.space1

    Text {
        anchors.verticalCenter: parent.verticalCenter
        text: Math.round(root.percent) + "%"
        color: Theme.text
        font.pixelSize: Theme.fontLabel
    }
    Item {
        anchors.verticalCenter: parent.verticalCenter
        width: 22; height: 11
        Rectangle {
            id: body
            width: 20; height: 11; radius: 3
            color: "transparent"
            border.color: Theme.text
            border.width: 1.5
            Rectangle {
                x: 2.5; y: 2.5
                height: parent.height - 5
                width: Math.max(1.5, (parent.width - 5) * Math.min(1, root.percent / 100))
                radius: 1
                color: root.charging ? Theme.success : root.percent <= 15 ? Theme.danger : Theme.text
            }
        }
        Rectangle { x: 20.5; y: 3.5; width: 1.5; height: 4; color: Theme.text; radius: 0.5 }
    }
    Icon {
        anchors.verticalCenter: parent.verticalCenter
        visible: root.charging
        name: "plug"
        size: Theme.iconSmall - 2
    }
}
