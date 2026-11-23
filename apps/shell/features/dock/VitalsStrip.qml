import QtQuick
import "../../components"
import "../../state"
import "../../theme"

// Laptop vitals on Ambient Home (live topics "host" and "system"): battery, lock state,
// CPU and memory. Unknown values are left out; a stale system sample (no update within
// 3× its interval) is dimmed. Tap → the Laptop view.
Pressable {
    id: root
    property var host: null      // {data: {locked, battery: {percent, charging}}}
    property var system: null    // {data: {cpu_percent, mem_percent}, stale}
    readonly property var h: host ? host.data : null
    readonly property var s: system ? system.data : null
    pressScale: 0.99
    implicitHeight: 56
    onTapped: Shell.dockNav.navigate("link")

    Rectangle {
        anchors.fill: parent
        radius: Theme.radius
        color: root.pressed ? Theme.surfacePressed : Theme.surface
    }
    Row {
        id: who
        anchors.left: parent.left
        anchors.leftMargin: Theme.space4
        anchors.verticalCenter: parent.verticalCenter
        spacing: Theme.space2
        Icon { anchors.verticalCenter: parent.verticalCenter; name: "laptop"; size: Theme.icon }
        Icon {
            anchors.verticalCenter: parent.verticalCenter
            visible: root.h !== null && root.h.locked === true
            name: "lock"
            size: Theme.iconSmall
            opacity: 0.8
        }
        BatteryIndicator {
            anchors.verticalCenter: parent.verticalCenter
            available: root.h !== null && root.h.battery !== undefined && root.h.battery !== null
            percent: available ? root.h.battery.percent : 0
            charging: available && root.h.battery.charging === true
        }
    }
    Row {
        anchors.left: who.right
        anchors.leftMargin: Theme.space5
        anchors.right: parent.right
        anchors.rightMargin: Theme.space4
        anchors.verticalCenter: parent.verticalCenter
        spacing: Theme.space4
        visible: root.s !== null
        readonly property real meterWidth: (width - spacing) / 2
        Meter {
            width: parent.meterWidth
            label: "CPU"
            value: root.s ? root.s.cpu_percent : 0
            stale: root.system !== null && root.system.stale === true
        }
        Meter {
            width: parent.meterWidth
            label: "MEMORY"
            value: root.s ? root.s.mem_percent : 0
            stale: root.system !== null && root.system.stale === true
            color: Theme.categoryColor("apps")
        }
    }
}
