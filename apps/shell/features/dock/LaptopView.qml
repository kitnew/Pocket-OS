pragma ComponentBehavior: Bound
import QtQuick
import "../../components"
import "../../state"
import "../../theme"

// Laptop view (rail ▸ Laptop; nav id "link"): the connection and what the laptop allows.
//
//   left   the laptop: name, state, reconnect countdown or error, Connect / Retry now /
//          Disconnect, Leave Dock (always available, also without a laptop)
//   right  Permissions: every elevated action the laptop offers, allowed or not, with
//          "Ask" (an Allow/Deny prompt on the laptop, access.request). Granting happens
//          only on the laptop.
// Everything comes from Pocket Core's Dock session (Shell.dock).
Item {
    id: root
    readonly property var dock: Shell.dock
    readonly property bool connected: Shell.dockConnected
    readonly property string host: dock.hostName || "Laptop"
    readonly property var elevated: dock.actions.filter(a => a.privilege === "elevated")
    readonly property color stateColor: !dock.supported ? Theme.textDisabled
        : connected ? Theme.success
        : dock.state === "connecting" || dock.retryScheduled ? Theme.warning
        : dock.state === "error" ? Theme.danger : Theme.textMuted

    readonly property string stateText: {
        if (!Shell.coreConnected)
            return "Pocket Core is not running"
        if (!dock.supported)
            return "This Pocket Core has no Dock support"
        if (!dock.configured)
            return "No laptop paired"
        if (connected)
            return "Connected"
        if (dock.state === "connecting")
            return "Connecting…"
        if (dock.state === "error")
            return dock.retryScheduled ? (dock.retrySeconds > 0 ? "Reconnecting in " + dock.retrySeconds + " s" : "Reconnecting…")
                                       : "Not connected"
        return "Not connected"
    }
    readonly property string helpText: {
        if (!Shell.coreConnected || !dock.supported)
            return ""
        if (!dock.configured)
            return "On the laptop, run scripts/dock-pair.sh from the Pocket OS repository."
        if (dock.state === "error")
            return dock.errorText
        if (dock.state === "disconnected")
            return "Disconnected. Connect when you want the Deck and live state back."
        return ""
    }

    DockStatus {
        anchors.right: parent.right
        anchors.rightMargin: Theme.gutter
        y: Theme.space3 + 6
        showTime: true
    }

    // ── The laptop ───────────────────────────────────────────────────────────────────
    Column {
        id: side
        x: Theme.gutter
        y: Theme.space5
        width: (root.width - 3 * Theme.gutter) * 0.45
        spacing: Theme.space3

        Item {
            width: 72; height: 72
            Rectangle {
                anchors.fill: parent
                radius: 22
                color: Theme.soft(root.stateColor, 0.16)
                border.width: 1.5
                border.color: Theme.soft(root.stateColor, 0.6)
            }
            Icon { anchors.centerIn: parent; name: root.dock.configured ? "laptop" : "link-off"; size: 34 }
            Rectangle {
                anchors.right: parent.right
                anchors.bottom: parent.bottom
                anchors.margins: -2
                width: 18; height: 18; radius: 9
                color: root.stateColor
                border.width: 3
                border.color: Theme.background
            }
        }
        Column {
            spacing: 2
            width: parent.width
            Text {
                width: parent.width
                text: root.dock.configured ? root.host : "Laptop"
                color: Theme.text
                font.pixelSize: Theme.fontHeadline
                font.weight: Font.DemiBold
                elide: Text.ElideRight
            }
            Text {
                text: root.stateText
                color: root.stateColor
                font.pixelSize: Theme.fontBody
                font.weight: Font.Medium
            }
            Text {
                width: parent.width
                visible: text.length > 0
                topPadding: 2
                text: root.helpText
                wrapMode: Text.Wrap
                maximumLineCount: 3
                elide: Text.ElideRight
                color: Theme.textMuted
                font.pixelSize: Theme.fontLabel
            }
            Text {
                visible: root.connected
                topPadding: 2
                text: root.dock.address + " · key " + root.dock.hostId
                color: Theme.textDisabled
                font.pixelSize: Theme.fontCaption
            }
        }
    }
    Flow {
        anchors.left: side.left
        anchors.bottom: parent.bottom
        anchors.bottomMargin: Theme.gutter
        width: side.width
        spacing: Theme.space2
        PocketButton {
            visible: root.dock.supported && root.dock.configured
            variant: root.connected || root.dock.state === "connecting" ? "tonal" : "primary"
            icon: root.connected || root.dock.state === "connecting" ? "link-off" : "refresh"
            text: root.dock.state === "disconnected" ? "Connect"
                : root.dock.state === "error" ? "Retry now" : "Disconnect"
            onActivated: root.dock.state === "disconnected" || root.dock.state === "error"
                         ? Shell.connectDock() : Shell.disconnectDock()
        }
        PocketButton {
            variant: "tonal"
            icon: "exit"
            text: "Leave Dock"
            enabled: Shell.coreConnected && !Shell.core.requestPending
            onActivated: Shell.requestMode("handheld")
        }
    }

    // ── Permissions ──────────────────────────────────────────────────────────────────
    Rectangle {
        id: perms
        anchors.right: parent.right
        anchors.rightMargin: Theme.gutter
        anchors.top: parent.top
        anchors.topMargin: 56
        anchors.bottom: parent.bottom
        anchors.bottomMargin: Theme.gutter
        width: (root.width - 3 * Theme.gutter) * 0.55
        radius: Theme.radiusLarge
        color: Theme.surface

        Text {
            id: permsTitle
            x: Theme.space4
            y: Theme.space3
            text: "Permissions on " + root.host
            color: Theme.text
            font.pixelSize: Theme.fontBody
            font.weight: Font.DemiBold
        }
        Text {
            id: permsSub
            anchors.top: permsTitle.bottom
            x: Theme.space4
            width: parent.width - 2 * Theme.space4
            text: "Granted on the laptop only. Media and volume are always allowed."
            color: Theme.textMuted
            font.pixelSize: Theme.fontCaption
            elide: Text.ElideRight
        }
        ListView {
            id: list
            anchors.top: permsSub.bottom
            anchors.topMargin: Theme.space2
            anchors.left: parent.left
            anchors.right: parent.right
            anchors.bottom: parent.bottom
            anchors.margins: Theme.space2
            anchors.leftMargin: Theme.space3
            anchors.rightMargin: Theme.space3
            clip: true
            spacing: 0
            boundsBehavior: Flickable.StopAtBounds
            model: root.connected ? root.elevated : []
            delegate: Item {
                id: row
                required property var modelData
                readonly property bool granted: !/^not granted/i.test(modelData.reason || "")
                readonly property bool asked: Shell.accessAsked[modelData.id] !== undefined
                                              && Date.now() - Shell.accessAsked[modelData.id] < 120000
                width: list.width
                height: 44
                Icon {
                    id: ic
                    anchors.verticalCenter: parent.verticalCenter
                    name: row.granted ? "check" : "lock"
                    size: 18
                    opacity: row.granted ? 1 : 0.6
                }
                Column {
                    anchors.left: ic.right
                    anchors.leftMargin: Theme.space3
                    anchors.right: action.left
                    anchors.rightMargin: Theme.space2
                    anchors.verticalCenter: parent.verticalCenter
                    Text {
                        width: parent.width
                        text: row.modelData.label
                        color: Theme.text
                        font.pixelSize: Theme.fontLabel
                        font.weight: Font.DemiBold
                        elide: Text.ElideRight
                    }
                    Text {
                        width: parent.width
                        text: row.granted ? (row.modelData.available ? "Allowed" : "Allowed · " + Shell.humanReason(row.modelData.reason))
                            : row.asked ? "Waiting for Allow on " + root.host : "Not allowed"
                        color: row.granted ? Theme.success : row.asked ? Theme.warning : Theme.textMuted
                        font.pixelSize: Theme.fontCaption
                        elide: Text.ElideRight
                    }
                }
                PocketButton {
                    id: action
                    anchors.right: parent.right
                    anchors.verticalCenter: parent.verticalCenter
                    visible: !row.granted && Shell.canRequest(row.modelData.id)
                    compact: true
                    variant: "primary"
                    enabled: !row.asked
                    text: row.asked ? "Asked" : "Ask"
                    onActivated: Shell.requestAccess(row.modelData.id)
                }
            }
            Text {
                anchors.centerIn: parent
                width: parent.width - 2 * Theme.space3
                horizontalAlignment: Text.AlignHCenter
                wrapMode: Text.Wrap
                visible: list.count === 0
                text: root.connected ? root.host + " offers no actions that need permission."
                                     : "Connect to see what " + root.host + " allows."
                color: Theme.textMuted
                font.pixelSize: Theme.fontLabel
            }
        }
    }
}
