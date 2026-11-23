import QtQuick
import "../../state"
import "../../theme"

// Ambient Home: the default Dock view. Large clock and date, the laptop link and the
// phone's battery on the left; adaptive widgets on the right, each shown only when its
// source exists and is current (docs/dock-mode/ux-specification.md). Remote widgets read
// Core's live topics, which Core clears when the session ends: nothing stale is shown.
// No animation, no polling: the clock changes once per minute (Shell.now).
Item {
    id: root
    readonly property var dock: Shell.dock
    readonly property var sys: Shell.system
    readonly property bool connected: dock.supported && dock.state === "connected"
    readonly property var live: dock.live
    function topic(name) {
        const t = live[name]
        return t && t.available ? t : null
    }
    readonly property var media: topic("media")
    readonly property bool hasMedia: connected && media !== null && media.data
                                     && (media.data.players || []).length > 0
    readonly property bool hasLaptop: connected && (topic("host") !== null || topic("system") !== null)

    readonly property string linkText: {
        if (!Shell.coreConnected)
            return "Pocket Core is not connected"
        if (!dock.supported || !dock.configured)
            return "No laptop paired"
        const host = dock.hostName || "Laptop"
        if (dock.state === "connected")
            return host + " · Connected"
        if (dock.state === "connecting")
            return host + " · Connecting…"
        if (dock.state === "error")
            return host + " · " + (dock.retryScheduled ? (dock.retrySeconds > 0 ? "Reconnecting in " + dock.retrySeconds + " s"
                                                                                  : "Reconnecting…")
                                                       : "Not connected")
        return host + " · Not connected"
    }
    readonly property color linkColor: dock.state === "connected" ? Theme.success
        : dock.state === "connecting" || dock.retryScheduled ? Theme.warning
        : dock.state === "error" ? Theme.danger : Theme.textMuted

    // ── Left: time and status ────────────────────────────────────────────────────────
    Column {
        id: leftColumn
        anchors.left: parent.left
        anchors.leftMargin: Theme.space5
        anchors.verticalCenter: parent.verticalCenter
        width: root.hasMedia || root.hasLaptop || Shell.dockNotifications.count > 0
               ? parent.width * 0.46 : parent.width - 2 * Theme.space5
        spacing: 0

        Text {
            text: Qt.formatTime(Shell.now, "HH:mm")
            color: Theme.text
            font.pixelSize: 88
            font.weight: Font.Light
            lineHeight: 0.9
        }
        Text {
            text: Qt.formatDate(Shell.now, "dddd, d MMMM")
            color: Theme.textMuted
            font.pixelSize: Theme.fontTitle
        }
        Item { width: 1; height: Theme.space4 }
        Row {
            spacing: Theme.space2
            Rectangle {
                anchors.verticalCenter: parent.verticalCenter
                width: 8; height: 8; radius: 4
                color: root.linkColor
            }
            Text {
                text: root.linkText
                color: Theme.text
                font.pixelSize: Theme.fontLabel
            }
        }
        Item { width: 1; height: Theme.space1 }
        Text {
            visible: root.sys !== null && root.sys.batteryAvailable
            text: root.sys ? "Phone " + Math.round(root.sys.batteryPercent) + "%"
                             + (root.sys.batteryCharging ? " · charging" : root.sys.batteryFull ? " · full" : "") : ""
            color: Theme.textMuted
            font.pixelSize: Theme.fontLabel
        }
    }

    // ── Right: adaptive widgets ─────────────────────────────────────────────────────
    Column {
        anchors.right: parent.right
        anchors.rightMargin: Theme.space4
        anchors.left: leftColumn.right
        anchors.leftMargin: Theme.space4
        anchors.verticalCenter: parent.verticalCenter
        spacing: Theme.space2

        MediaWidget {
            width: parent.width
            visible: root.hasMedia
            media: root.media ? root.media.data : null
        }
        LaptopWidget {
            width: parent.width
            visible: root.hasLaptop
            host: root.topic("host")
            system: root.topic("system")
        }
        NotificationsWidget {
            width: parent.width
            visible: Shell.dockNotifications.count > 0
        }
    }
}
