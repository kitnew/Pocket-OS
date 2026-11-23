import QtQuick
import "../../components"
import "../../state"
import "../../theme"

// The Dock's own compact status (the system status bar is hidden on the immersive Dock):
// laptop notifications (tap → history), phone battery, optionally the time (views other
// than Ambient Home, which shows the large clock). Nothing here duplicates another item
// on the same screen.
Row {
    id: root
    property bool showTime: false
    readonly property var sys: Shell.system
    spacing: Theme.space3

    Pressable {
        anchors.verticalCenter: parent.verticalCenter
        visible: Shell.dockNotifications.count > 0 || Shell.dock.preferences.dnd === true
        width: bellRow.implicitWidth + 2 * Theme.space2
        height: 32
        onTapped: Shell.dockNav.openModal("notifications")
        Accessible.role: Accessible.Button
        Accessible.name: "Laptop notifications"
        Rectangle {
            anchors.fill: parent
            radius: height / 2
            color: parent.pressed ? Theme.surfacePressed : Theme.surfaceRaised
        }
        Row {
            id: bellRow
            anchors.centerIn: parent
            spacing: Theme.space1
            Icon {
                anchors.verticalCenter: parent.verticalCenter
                name: Shell.dock.preferences.dnd === true ? "moon" : "bell"
                size: Theme.iconSmall
            }
            Text {
                anchors.verticalCenter: parent.verticalCenter
                visible: Shell.dockNotifications.count > 0
                text: Shell.dockNotifications.count
                color: Theme.text
                font.pixelSize: Theme.fontLabel
                font.weight: Font.DemiBold
            }
        }
    }
    Icon {
        anchors.verticalCenter: parent.verticalCenter
        visible: root.sys !== null && root.sys.wifiAvailable && root.sys.wifiConnected
        name: "wifi"
        size: Theme.iconSmall
        opacity: 0.8
    }
    BatteryIndicator {
        anchors.verticalCenter: parent.verticalCenter
        available: root.sys !== null && root.sys.batteryAvailable
        percent: root.sys ? root.sys.batteryPercent : 0
        charging: root.sys !== null && root.sys.batteryCharging
    }
    Text {
        anchors.verticalCenter: parent.verticalCenter
        visible: root.showTime
        text: Qt.formatTime(Shell.now, "HH:mm")
        color: Theme.text
        font.pixelSize: Theme.fontBody
        font.weight: Font.DemiBold
        font.features: { "tnum": 1 }
    }
}
