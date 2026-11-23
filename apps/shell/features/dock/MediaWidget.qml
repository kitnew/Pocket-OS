import QtQuick
import "../../components"
import "../../state"
import "../../theme"

// Now playing on the laptop (live topic "media", MPRIS on the laptop). Shows the active
// player; the buttons run the agent's media actions on that player. Hidden by the parent
// when there is no player or no current data.
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
    function available(id) {
        const a = Shell.dockAction(id)
        return a !== null && a.available && Shell.dock.pendingActions[id] === undefined
    }

    implicitHeight: 104
    radius: Theme.radius
    color: Theme.surfaceRaised

    Column {
        anchors.left: parent.left
        anchors.right: controls.left
        anchors.verticalCenter: parent.verticalCenter
        anchors.leftMargin: Theme.space3
        anchors.rightMargin: Theme.space2
        spacing: 2
        Text {
            width: parent.width
            text: root.player ? ((root.player.status || "Unknown") + " · " + (root.player.identity || root.player.id)).toUpperCase() : ""
            color: root.playing ? Theme.success : Theme.textMuted
            font.pixelSize: Theme.fontCaption
            font.letterSpacing: 0.5
            elide: Text.ElideRight
        }
        Text {
            width: parent.width
            text: root.player ? (root.player.title || "Unknown title") : ""
            color: Theme.text
            font.pixelSize: Theme.fontBody
            font.weight: Font.DemiBold
            elide: Text.ElideRight
        }
        Text {
            width: parent.width
            visible: text.length > 0
            text: root.player ? (root.player.artist || "") : ""
            color: Theme.textMuted
            font.pixelSize: Theme.fontLabel
            elide: Text.ElideRight
        }
    }
    Row {
        id: controls
        anchors.right: parent.right
        anchors.rightMargin: Theme.space1
        anchors.verticalCenter: parent.verticalCenter
        IconButton {
            icon: "skip-back"
            accessibleName: "Previous track"
            enabled: root.available("media.previous") && root.player !== null && root.player.can_previous !== false
            onActivated: Shell.runDockAction("media.previous")
        }
        IconButton {
            icon: root.playing ? "pause" : "play"
            accessibleName: "Play or pause"
            flat: false
            enabled: root.available("media.play_pause")
            onActivated: Shell.runDockAction("media.play_pause")
        }
        IconButton {
            icon: "skip-forward"
            accessibleName: "Next track"
            enabled: root.available("media.next") && root.player !== null && root.player.can_next !== false
            onActivated: Shell.runDockAction("media.next")
        }
    }
}
