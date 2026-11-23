import QtQuick
import Pocket.Core
import "../../components"
import "../../state"
import "../../theme"

// Development diagnostics (opened from Quick Settings → Developer): live link to Pocket
// Core, the logical mode switch, platform adapter states, last diagnostic event. The
// displayed mode is always the core's (GetState/StateChanged), never a local guess.
// Uses the shared Shell.core connection.
Rectangle {
    id: root

    readonly property CoreClient core: Shell.core
    readonly property bool connected: Shell.coreConnected
    readonly property string otherMode: root.core.mode === "dock" ? "handheld" : "dock"
    readonly property bool landscape: width > height
    signal closeRequested()

    color: Theme.background
    function title(s) { return s.length ? s.charAt(0).toUpperCase() + s.slice(1) : "—" }
    function yn(b) { return b ? "yes" : "no" }

    IconButton {
        anchors.top: parent.top
        anchors.right: parent.right
        anchors.margins: Theme.space2
        icon: "close"
        accessibleName: "Close developer panel"
        onActivated: root.closeRequested()
    }

    Flickable {
        anchors.fill: parent
        anchors.margins: Theme.space5
        anchors.topMargin: Theme.space6
        contentHeight: column.implicitHeight
        clip: true
        boundsBehavior: Flickable.StopAtBounds

        Column {
            id: column
            width: parent.width
            spacing: Theme.space4

            Text {
                text: "Developer"
                color: Theme.text
                font.pixelSize: Theme.fontTitle
                font.weight: Font.DemiBold
            }
            Row {
                spacing: Theme.space2
                Rectangle {
                    anchors.verticalCenter: parent.verticalCenter
                    width: 12; height: 12; radius: 6
                    color: root.core.status === CoreClient.Connected ? Theme.success
                         : root.core.status === CoreClient.Connecting ? Theme.warning : Theme.danger
                }
                Text {
                    text: "Core: " + root.core.statusText
                    color: Theme.text
                    font.pixelSize: Theme.fontBody
                }
            }
            Text {
                text: "Mode: " + root.title(root.core.mode) + (root.connected ? "" : " (last known)")
                color: root.connected ? Theme.text : Theme.textMuted
                font.pixelSize: Theme.fontTitle
            }
            PocketButton {
                enabled: root.connected && !root.core.requestPending
                text: "Switch to " + root.title(root.otherMode)
                checked: root.core.mode === "dock"
                onActivated: (point) => {
                    const target = root.otherMode
                    Shell.diag("button.activated", {
                        request: target,
                        x: point.position.x.toFixed(0),
                        y: point.position.y.toFixed(0),
                        device: point.device ? point.device.name : "unknown"
                    })
                    root.core.requestMode(target)
                }
            }
            Text {
                width: parent.width
                wrapMode: Text.Wrap
                color: Theme.textMuted
                font.pixelSize: Theme.fontLabel
                text: (root.connected
                       ? "pocketd " + root.core.coreVersion + " · protocol " + root.core.protocol
                         + " · rev " + root.core.revision + " · " + root.core.owner
                       : (root.core.lastError || "waiting for pocketd"))
                      + "\nlinks " + root.core.connections + " · apps " + Shell.apps.count
                      + (Shell.apps.lastError ? " (" + Shell.apps.lastError + ")" : "")
                      + "\nwindows " + Shell.windowCount + " · home " + Shell.homeVisible + "/" + Shell.homePage
                      + "\nsurface " + root.width + "×" + root.height + (root.landscape ? " landscape" : " portrait")
                      + "\noutput " + (Shell.output.available ? Shell.output.outputName + " rot " + Shell.output.rotation : "n/a")
                      + " · display " + (Shell.display.available ? (Shell.display.on ? "on" : "off") : "n/a")
                      + "\npower key grab " + root.yn(Shell.powerKey.active) + (Shell.powerKey.error ? " (" + Shell.powerKey.error + ")" : "")
                      + "\nbacklight " + (Shell.brightness.available ? Shell.brightness.device + " " + Math.round(Shell.brightness.value * 100) + "%" : "n/a")
                      + "\naccelerometer " + root.yn(Shell.sensor.available) + " claimed " + root.yn(Shell.sensor.claimed)
                      + " " + Shell.sensor.orientation + (Shell.sensor.lastError ? " (" + Shell.sensor.lastError + ")" : "")
                      + "\nosk " + root.yn(Shell.osk.available) + " visible " + root.yn(Shell.osk.visible)
                      + " · sound cards " + Shell.audio.cards
            }
            Text {
                width: parent.width
                wrapMode: Text.WrapAnywhere
                text: "last: " + Shell.lastEvent
                color: Theme.textMuted
                font.pixelSize: Theme.fontCaption
            }
        }
    }
}
