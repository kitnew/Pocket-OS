pragma ComponentBehavior: Bound
import QtQuick
import "../../components"
import "../../state"
import "../../theme"

// Dock Mode control surface (landscape). Everything shown comes from Pocket Core's Dock
// session (Shell.dock): connection state, the computer's identity, and the actions the
// connected desktop agent advertises. Taps go through Core; a result is shown only
// after the computer confirmed it. Works, and can be left, without any computer.
Item {
    id: root
    readonly property var dock: Shell.dock
    readonly property bool connected: dock.supported && dock.state === "connected"
    readonly property color stateColor: !dock.supported ? Theme.textDisabled
        : dock.state === "connected" ? Theme.success
        : dock.state === "connecting" ? Theme.warning
        : dock.state === "error" ? Theme.danger : Theme.textMuted

    function title(s) { return s.length ? s.charAt(0).toUpperCase() + s.slice(1) : "" }

    // What to say when no actions can be shown.
    readonly property string message: {
        if (!Shell.coreConnected)
            return "Pocket Core is not connected."
        if (!dock.supported)
            return "This Pocket Core has no Dock support."
        if (!dock.configured)
            return "No computer is paired. On the computer, run scripts/dock-pair.sh."
        if (dock.state === "connecting")
            return "Connecting to " + dock.hostName + " (" + dock.address + ")…"
        if (dock.state === "error")
            return dock.errorText + (dock.retryIn > 0 ? "\nRetrying in " + dock.retryIn + " s." : "")
        if (dock.state === "disconnected")
            return "Not connected to " + dock.hostName + "."
        return dock.actions.length === 0 ? dock.hostName + " offers no actions." : ""
    }

    Column {
        anchors.fill: parent
        anchors.margins: Theme.space4
        anchors.bottomMargin: Theme.space3
        spacing: Theme.space3

        // ── Header: title and connection ──────────────────────────────────────
        Item {
            width: parent.width
            height: Theme.touchTarget * 0.8
            Row {
                anchors.verticalCenter: parent.verticalCenter
                spacing: Theme.space2
                Icon { anchors.verticalCenter: parent.verticalCenter; name: "dock" }
                Text {
                    anchors.verticalCenter: parent.verticalCenter
                    text: "Pocket Dock"
                    color: Theme.text
                    font.pixelSize: Theme.fontTitle
                    font.weight: Font.DemiBold
                }
            }
            Row {
                anchors.right: parent.right
                anchors.verticalCenter: parent.verticalCenter
                spacing: Theme.space2
                Rectangle {
                    anchors.verticalCenter: parent.verticalCenter
                    width: 10; height: 10; radius: 5
                    color: root.stateColor
                }
                Text {
                    anchors.verticalCenter: parent.verticalCenter
                    text: !root.dock.supported ? "Unavailable"
                        : (root.dock.hostName ? root.dock.hostName + " · " : "")
                          + (root.dock.state === "error" ? "Not connected" : root.title(root.dock.state))
                    color: Theme.text
                    font.pixelSize: Theme.fontLabel
                    font.weight: Font.Medium
                }
            }
        }

        // ── Body: actions, or why there are none ──────────────────────────────
        Item {
            id: body
            width: parent.width
            height: parent.height - y - footer.height - parent.spacing

            Flickable {
                anchors.fill: parent
                visible: root.connected && root.dock.actions.length > 0
                contentHeight: grid.height
                clip: true
                boundsBehavior: Flickable.StopAtBounds
                Grid {
                    id: grid
                    width: parent.width
                    columns: Math.max(1, Math.min(4, Math.floor(width / 170)))
                    spacing: Theme.space2
                    readonly property real cellWidth: (width - (columns - 1) * spacing) / columns
                    Repeater {
                        model: root.dock.actions
                        delegate: DockActionButton {
                            required property var modelData
                            width: grid.cellWidth
                            label: modelData.label || modelData.id
                            category: modelData.category || ""
                            available: modelData.available
                            reason: modelData.reason || "Unavailable"
                            busy: root.dock.pendingActions[modelData.id] !== undefined
                            result: Shell.dockResults[modelData.id] || null
                            onActivated: Shell.runDockAction(modelData.id)
                        }
                    }
                }
            }
            Text {
                anchors.fill: parent
                visible: root.message.length > 0
                horizontalAlignment: Text.AlignHCenter
                verticalAlignment: Text.AlignVCenter
                wrapMode: Text.Wrap
                text: root.message
                color: root.dock.state === "error" ? Theme.danger : Theme.textMuted
                font.pixelSize: Theme.fontBody
            }
        }

        // ── Footer: leave Dock Mode, connection control ───────────────────────
        Item {
            id: footer
            width: parent.width
            height: Theme.touchTarget
            PocketButton {
                anchors.left: parent.left
                text: "Back to Handheld"
                enabled: Shell.coreConnected && !Shell.core.requestPending
                onActivated: Shell.requestMode("handheld")
            }
            Text {
                anchors.centerIn: parent
                width: parent.width / 3
                horizontalAlignment: Text.AlignHCenter
                visible: root.connected
                text: root.dock.address + " · " + root.dock.hostId
                color: Theme.textDisabled
                font.pixelSize: Theme.fontCaption
                elide: Text.ElideMiddle
            }
            PocketButton {
                anchors.right: parent.right
                visible: root.dock.supported && root.dock.configured
                primary: root.dock.state === "disconnected" || root.dock.state === "error"
                text: root.dock.state === "disconnected" ? "Connect"
                    : root.dock.state === "error" ? "Retry now" : "Disconnect"
                onActivated: root.dock.state === "disconnected" || root.dock.state === "error"
                             ? Shell.connectDock() : Shell.disconnectDock()
            }
        }
    }
}
