import QtQuick
import "../../components"
import "../../state"
import "../../theme"

// One Control Deck tile. Its look comes from Shell.tileState() (state/deckmodel.js):
// real Core/laptop state only, never a local guess.
//   tap        run (or Retry an unknown outcome; or explain why it cannot run)
//   long press details: what it does, its state, how to allow/set it up
//   edit mode  tap opens the tile editor; nothing runs
Pressable {
    id: root
    property var tile                 // {id, action, label, icon, params}
    property bool editing: false
    property bool showResult: false
    readonly property var action: Shell.dock.actions.length >= 0 ? Shell.dockAction(tile ? tile.action : "") : null
    readonly property var result: tile ? Shell.tileResults[tile.id] || null : null
    readonly property var st: tile ? Shell.tileState(tile, showResult) : ({ kind: "missing", status: "", tappable: false })
    readonly property string kind: st.kind
    readonly property bool dim: kind === "offline" || kind === "missing" || kind === "unavailable"
    readonly property color hue: Theme.categoryColor(action ? action.category : (tile ? tile.action.split(".")[0] : ""))

    interactive: editing || st.tappable
    longPressEnabled: !editing && tile !== null
    pressScale: 0.95
    Accessible.role: Accessible.Button
    Accessible.name: tile ? tile.label + (st.status ? ", " + st.status : "") : ""

    onTapped: {
        if (editing)
            Shell.editDeckTile(tile)
        else if (kind === "uncertain")
            Shell.retryDeckTile(tile)
        else
            Shell.runDeckTile(tile)
    }
    onLongPressed: Shell.openTileDetail(tile)

    onResultChanged: {
        showResult = result !== null && result.ok && !result.progress
        if (showResult)
            fade.restart()
    }
    Timer { id: fade; interval: 2500; onTriggered: root.showResult = false }

    Rectangle {
        id: bg
        anchors.fill: parent
        radius: Theme.radiusLarge - 4
        color: root.pressed ? Theme.surfacePressed
             : root.kind === "done" ? Qt.tint(Theme.surfaceRaised, Theme.successSoft)
             : root.dim ? Theme.surface : Theme.surfaceRaised
        border.width: root.kind === "running" || root.kind === "failed" || root.kind === "uncertain" || root.editing ? 1.5 : 1
        border.color: root.editing ? Theme.outlineStrong
            : root.kind === "running" ? Theme.accent
            : root.kind === "failed" ? Theme.danger
            : root.kind === "uncertain" ? Theme.warning
            : root.dim ? Theme.outline : "transparent"
        Behavior on color { ColorAnimation { duration: Theme.duration } }
    }

    Item {
        anchors.fill: parent
        anchors.margins: Theme.space3
        opacity: root.dim ? 0.45 : 1
        Behavior on opacity { NumberAnimation { duration: Theme.duration } }

        Rectangle {
            id: well
            width: 42; height: 42
            radius: 13
            color: Theme.soft(root.hue, root.kind === "unauthorized" || root.kind === "unconfigured" ? 0.08 : 0.18)
            Icon {
                anchors.centerIn: parent
                name: root.tile && root.tile.icon ? root.tile.icon : "action"
                size: Theme.icon
                opacity: root.kind === "unauthorized" || root.kind === "unconfigured" ? 0.55 : 1
                onStatusChanged: if (status === Image.Error) name = "action"
            }
        }

        // State badge (top right).
        Item {
            anchors.right: parent.right
            anchors.top: parent.top
            width: 24; height: 24
            Spinner {
                anchors.centerIn: parent
                size: 18
                visible: root.kind === "running"
            }
            Rectangle {
                anchors.fill: parent
                radius: 12
                visible: badge.name !== ""
                color: root.kind === "done" ? Theme.success
                     : root.kind === "failed" ? Theme.danger
                     : root.kind === "uncertain" ? Theme.warning
                     : root.editing ? Theme.surfaceHigh : Theme.surfaceHigh
                Icon {
                    id: badge
                    anchors.centerIn: parent
                    size: 14
                    name: root.editing ? "edit"
                        : root.kind === "done" ? "check"
                        : root.kind === "failed" ? "alert"
                        : root.kind === "uncertain" ? "refresh"
                        : root.kind === "unauthorized" ? "lock"
                        : root.kind === "unconfigured" ? "info" : ""
                }
            }
        }

        Column {
            anchors.left: parent.left
            anchors.right: parent.right
            anchors.bottom: parent.bottom
            spacing: 1
            Text {
                width: parent.width
                text: root.tile ? root.tile.label : ""
                color: Theme.text
                font.pixelSize: Theme.fontBody
                font.weight: Font.DemiBold
                elide: Text.ElideRight
            }
            Text {
                width: parent.width
                visible: text.length > 0
                text: root.editing ? (root.st.kind === "ready" ? "Tap to edit" : root.st.status) : root.st.status
                color: root.kind === "failed" ? Theme.danger
                     : root.kind === "uncertain" ? Theme.warning
                     : root.kind === "done" ? Theme.success
                     : root.kind === "running" ? Theme.accent : Theme.textMuted
                font.pixelSize: Theme.fontCaption
                elide: Text.ElideRight
            }
        }
    }
}
