import QtQuick
import "../../components"
import "../../state"
import "../../theme"

// Status bar content: time, Pocket indicators, network, Bluetooth, battery. Every
// indicator reflects real state from Shell.system / Shell.core and is hidden when the
// source is unavailable. Tapping it toggles Quick Settings.
Rectangle {
    id: root
    color: Shell.panel === "quicksettings" ? Theme.surface : Theme.background
    readonly property var sys: Shell.system

    Text {
        anchors.left: parent.left
        anchors.leftMargin: Theme.space4
        anchors.verticalCenter: parent.verticalCenter
        text: Qt.formatTime(Shell.now, "HH:mm")
        color: Theme.text
        font.pixelSize: Theme.fontLabel
        font.weight: Font.DemiBold
    }

    Row {
        anchors.right: parent.right
        anchors.rightMargin: Theme.space4
        anchors.verticalCenter: parent.verticalCenter
        spacing: Theme.space3

        StatusIndicator {
            anchors.verticalCenter: parent.verticalCenter
            visible: !Shell.coreConnected
            icon: "alert"
            text: "Core"
        }
        StatusIndicator {
            anchors.verticalCenter: parent.verticalCenter
            visible: Shell.docked
            icon: "dock"
        }
        // Desktop link (independent of the mode): shown while connected or trying.
        StatusIndicator {
            anchors.verticalCenter: parent.verticalCenter
            visible: Shell.dock.supported && Shell.dock.state !== "disconnected" && Shell.dock.state !== ""
            icon: Shell.dock.state === "error" ? "alert" : "plug"
            dim: Shell.dock.state !== "connected"
        }
        StatusIndicator {
            anchors.verticalCenter: parent.verticalCenter
            visible: root.sys !== null && root.sys.bluetoothAvailable && root.sys.bluetoothEnabled
            icon: "bluetooth"
            dim: root.sys !== null && root.sys.bluetoothConnected === 0
        }
        StatusIndicator {
            anchors.verticalCenter: parent.verticalCenter
            visible: root.sys !== null && root.sys.wifiAvailable
            icon: root.sys && root.sys.wifiEnabled ? "wifi" : "wifi-off"
            dim: root.sys !== null && !root.sys.wifiConnected
        }
        BatteryIndicator {
            anchors.verticalCenter: parent.verticalCenter
            available: root.sys !== null && root.sys.batteryAvailable
            percent: root.sys ? root.sys.batteryPercent : 0
            charging: root.sys !== null && root.sys.batteryCharging
        }
    }

    TapHandler {
        gesturePolicy: TapHandler.ReleaseWithinBounds
        onTapped: Shell.togglePanel("quicksettings")
    }
    DragHandler {
        target: null
        xAxis.enabled: false
        onActiveChanged: if (!active && centroid.position.y - centroid.pressPosition.y > 12)
                             Shell.panel = "quicksettings"
    }
}
