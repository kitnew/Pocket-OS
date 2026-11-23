import QtQuick
import "../../components"
import "../../state"
import "../../theme"

// Now playing on the laptop (live topic "media", MPRIS there). Shows the active player;
// the buttons run the agent's media actions on it and are enabled only when the laptop
// says they can run. Hidden by the parent when there is no player.
Rectangle {
    id: root
    property var media: null
    readonly property var player: {
        if (!media || !media.players)
            return null
        for (const p of media.players)
            if (p.id === media.active)
                return p
        return media.players.length > 0 ? media.players[0] : null
    }
    readonly property bool playing: player !== null && player.status === "Playing"
    function can(id) {
        const a = Shell.dockAction(id)
        return a !== null && a.available
    }
    function busy(id) { return Shell.dock.pendingActions[id] !== undefined }
    // "com.github.th-ch.youtube-music" → "Youtube Music"; "firefox" → "Firefox".
    function playerName(p) {
        let n = p.identity || p.id || ""
        if (n.indexOf(".") >= 0)
            n = n.split(".").pop()
        n = n.replace(/[-_]+/g, " ").replace(/instance\d*$/i, "").trim()
        return n.replace(/\b\w/g, c => c.toUpperCase())
    }

    implicitHeight: 152
    radius: Theme.radiusLarge
    color: Theme.surfaceRaised

    Column {
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: parent.top
        anchors.margins: Theme.space4
        spacing: 3
        Row {
            spacing: Theme.space2
            Rectangle {
                anchors.verticalCenter: parent.verticalCenter
                width: 6; height: 6; radius: 3
                color: root.playing ? Theme.categoryColor("media") : Theme.textDisabled
            }
            Text {
                text: root.player ? (root.playing ? "Now playing" : "Paused") + " · " + root.playerName(root.player) : ""
                color: Theme.textMuted
                font.pixelSize: Theme.fontCaption
                font.weight: Font.Medium
                font.letterSpacing: 0.4
            }
        }
        Text {
            width: parent.width
            text: root.player ? (root.player.title || "Unknown title") : ""
            color: Theme.text
            font.pixelSize: Theme.fontTitle
            font.weight: Font.DemiBold
            elide: Text.ElideRight
        }
        Text {
            width: parent.width
            text: root.player ? (root.player.artist || " ") : ""
            color: Theme.textMuted
            font.pixelSize: Theme.fontLabel
            elide: Text.ElideRight
        }
    }
    Row {
        anchors.left: parent.left
        anchors.bottom: parent.bottom
        anchors.leftMargin: Theme.space3
        anchors.bottomMargin: Theme.space3
        spacing: Theme.space2
        IconButton {
            width: 48; height: 48
            icon: "skip-back"
            accessibleName: "Previous track"
            enabled: root.can("media.previous") && root.player !== null && root.player.can_previous !== false
            onActivated: Shell.runDockAction("media.previous")
        }
        IconButton {
            width: 52; height: 52
            anchors.verticalCenter: parent.verticalCenter
            icon: root.playing ? "pause" : "play"
            iconSize: Theme.iconLarge
            accessibleName: root.playing ? "Pause" : "Play"
            checked: true
            enabled: root.can("media.play_pause") && !root.busy("media.play_pause")
            onActivated: Shell.runDockAction("media.play_pause")
        }
        IconButton {
            width: 48; height: 48
            icon: "skip-forward"
            accessibleName: "Next track"
            enabled: root.can("media.next") && root.player !== null && root.player.can_next !== false
            onActivated: Shell.runDockAction("media.next")
        }
    }
}
