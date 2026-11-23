import QtQuick
import "../../components"
import "../../state"
import "../../theme"

// Ambient Home: the default Dock view, readable from a desk.
//
//   ┌──────────────────────────────────────────────────────────────┐
//   │ ● nikitapc · Connected                        🔔 2  ▭ 100%   │  status line
//   │                                                              │
//   │  23:09                          ┌──────────────────────────┐ │
//   │  Monday, 23 November            │ NOW PLAYING · YT Music   │ │  media card (only
//   │                                 │ Stuck in a Timeloop      │ │  with a player)
//   │                                 │ Animadrop   ⏮  ⏯  ⏭      │ │
//   │                                 └──────────────────────────┘ │
//   │  nikitapc  ▭ 100%   CPU ▂▂▂ 39%   Memory ▆▆▆ 72%   🔒        │  vitals (connected)
//   └──────────────────────────────────────────────────────────────┘
//
// With no player the clock centres itself; with no laptop the vitals strip disappears and
// the status line says why. Remote content comes from Core's live topics, which Core
// clears when the session ends: nothing stale is ever shown. The clock changes once per
// minute (Shell.now); nothing animates while idle.
Item {
    id: root
    readonly property var dock: Shell.dock
    readonly property bool connected: Shell.dockConnected
    function topic(name) {
        const t = dock.live[name]
        return t && t.available ? t : null
    }
    readonly property var media: topic("media")
    readonly property bool hasMedia: connected && media !== null && media.data
                                     && (media.data.players || []).length > 0
    readonly property var host: topic("host")
    readonly property var system: topic("system")
    readonly property bool hasVitals: connected && (host !== null || system !== null)

    readonly property string linkText: {
        if (!Shell.coreConnected)
            return "Pocket Core is not running"
        if (!dock.supported || !dock.configured)
            return "No laptop paired"
        const name = dock.hostName || "Laptop"
        if (dock.state === "connected")
            return name
        if (dock.state === "connecting")
            return name + " · connecting…"
        if (dock.state === "error" && dock.retryScheduled)
            return name + " · " + (dock.retrySeconds > 0 ? "reconnecting in " + dock.retrySeconds + " s" : "reconnecting…")
        return name + " · offline"
    }
    readonly property color linkColor: connected ? Theme.success
        : dock.state === "connecting" || dock.retryScheduled ? Theme.warning
        : dock.state === "error" ? Theme.danger : Theme.textDisabled

    // ── Status line ──────────────────────────────────────────────────────────────────
    Chip {
        id: link
        x: Theme.gutter
        y: Theme.space4
        text: root.linkText
        dotColor: root.linkColor
        tint: "transparent"
        textColor: root.connected ? Theme.text : Theme.textMuted
        interactive: true
        onTapped: Shell.dockNav.navigate("link")
    }
    DockStatus {
        anchors.right: parent.right
        anchors.rightMargin: Theme.gutter
        anchors.verticalCenter: link.verticalCenter
    }

    // ── Clock ────────────────────────────────────────────────────────────────────────
    Column {
        id: clock
        readonly property real centreX: (root.width - width) / 2
        x: root.hasMedia ? Theme.gutter + Theme.space2 : centreX
        anchors.verticalCenter: parent.verticalCenter
        anchors.verticalCenterOffset: root.hasVitals ? -14 : 0
        spacing: -Theme.space2
        Behavior on x { NumberAnimation { duration: Theme.durationSlow; easing.type: Theme.easing } }

        Text {
            text: Qt.formatTime(Shell.now, "HH:mm")
            color: Theme.text
            font.pixelSize: root.hasMedia ? Theme.fontHero - 16 : Theme.fontHero
            font.weight: Font.Light
            font.features: { "tnum": 1 }
            Behavior on font.pixelSize { NumberAnimation { duration: Theme.durationSlow } }
        }
        Text {
            width: parent.width
            horizontalAlignment: root.hasMedia ? Text.AlignLeft : Text.AlignHCenter
            leftPadding: root.hasMedia ? 6 : 0
            text: Qt.formatDate(Shell.now, "dddd, d MMMM")
            color: Theme.textMuted
            font.pixelSize: Theme.fontTitle
        }
    }

    // ── Media ────────────────────────────────────────────────────────────────────────
    MediaCard {
        anchors.right: parent.right
        anchors.rightMargin: Theme.gutter
        anchors.verticalCenter: clock.verticalCenter
        width: Math.min(360, root.width * 0.5)
        media: root.media ? root.media.data : null
        opacity: root.hasMedia ? 1 : 0
        visible: opacity > 0
        Behavior on opacity { NumberAnimation { duration: Theme.duration } }
    }

    // ── Laptop vitals ────────────────────────────────────────────────────────────────
    VitalsStrip {
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.bottom: parent.bottom
        anchors.margins: Theme.gutter
        host: root.host
        system: root.system
        opacity: root.hasVitals ? 1 : 0
        visible: opacity > 0
        Behavior on opacity { NumberAnimation { duration: Theme.duration } }
    }

    // Not connected: one calm line with the way forward (no stale data, no placeholders).
    Text {
        anchors.horizontalCenter: parent.horizontalCenter
        anchors.bottom: parent.bottom
        anchors.bottomMargin: Theme.space5
        visible: !root.connected && Shell.coreConnected && root.dock.supported
        text: !root.dock.configured ? "Pair a laptop with scripts/dock-pair.sh"
            : root.dock.state === "disconnected" ? "Tap the laptop name to connect" : ""
        color: Theme.textDisabled
        font.pixelSize: Theme.fontLabel
    }
}
