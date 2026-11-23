import QtQuick
import "../../components"
import "../../state"
import "../../theme"

// Laptop vitals (live topics "host" and "system"). Values that are unknown are left out;
// a stale system sample (no update within 3× its interval) is dimmed and labelled.
Rectangle {
    id: root
    property var host: null      // {data: {locked, battery: {percent, charging}}}
    property var system: null    // {data: {cpu_percent, mem_percent}, stale}
    readonly property var h: host ? host.data : null
    readonly property var s: system ? system.data : null

    implicitHeight: 56
    radius: Theme.radius
    color: Theme.surfaceRaised

    Row {
        anchors.left: parent.left
        anchors.leftMargin: Theme.space3
        anchors.verticalCenter: parent.verticalCenter
        spacing: Theme.space2
        Icon { anchors.verticalCenter: parent.verticalCenter; name: "laptop"; size: Theme.icon }
        Column {
            anchors.verticalCenter: parent.verticalCenter
            spacing: 1
            Text {
                text: {
                    const parts = [Shell.dock.hostName || "Laptop"]
                    if (root.h && root.h.locked === true)
                        parts.push("Locked")
                    if (root.h && root.h.battery)
                        parts.push(Math.round(root.h.battery.percent) + "%" + (root.h.battery.charging ? " charging" : ""))
                    return parts.join(" · ")
                }
                color: Theme.text
                font.pixelSize: Theme.fontLabel
                font.weight: Font.DemiBold
            }
            Text {
                visible: root.s !== null
                text: root.s ? "CPU " + Math.round(root.s.cpu_percent) + "% · Memory " + Math.round(root.s.mem_percent) + "%"
                               + (root.system.stale ? " · not updating" : "") : ""
                color: root.system && root.system.stale ? Theme.textDisabled : Theme.textMuted
                font.pixelSize: Theme.fontCaption
            }
        }
    }
}
