import QtQuick
import "../../components"
import "../../state"
import "../../theme"

// Tile details (long press, or a tap on a tile that cannot run): what the tile does,
// its state in plain words, and the way forward: Run / Retry, or — for actions the
// laptop has not allowed — "Ask nikitapc", which shows an Allow/Deny prompt on the
// laptop (access.request). The phone never grants anything itself.
Sheet {
    id: root
    readonly property var tile: Shell.dockDetail ? Shell.dockDetail.tile : null
    readonly property var action: tile ? Shell.dockAction(tile.action) : null
    readonly property var st: tile ? Shell.tileState(tile, false) : null
    readonly property string host: Shell.dock.hostName || "the laptop"
    readonly property bool asked: tile !== null && Shell.accessAsked[tile.action] !== undefined
                                  && Date.now() - Shell.accessAsked[tile.action] < 120000
    open: tile !== null
    cardWidth: Math.min(width - 2 * Theme.gutter, 520)
    onDismissed: Shell.closeTileDetail()

    Column {
        width: parent.width
        spacing: Theme.space4

        Row {
            width: parent.width
            spacing: Theme.space3
            Rectangle {
                width: 48; height: 48; radius: 15
                color: Theme.soft(Theme.categoryColor(root.action ? root.action.category : ""), 0.2)
                Icon { anchors.centerIn: parent; name: root.tile && root.tile.icon ? root.tile.icon : "action" }
            }
            Column {
                anchors.verticalCenter: parent.verticalCenter
                width: parent.width - 48 - Theme.space3
                spacing: 2
                Text {
                    width: parent.width
                    text: root.tile ? root.tile.label : ""
                    color: Theme.text
                    font.pixelSize: Theme.fontTitle
                    font.weight: Font.DemiBold
                    elide: Text.ElideRight
                }
                Text {
                    width: parent.width
                    text: (root.action ? root.action.label : root.tile ? root.tile.action : "")
                          + (root.action && root.action.privilege === "elevated" ? " · needs permission on the laptop" : "")
                    color: Theme.textMuted
                    font.pixelSize: Theme.fontLabel
                    elide: Text.ElideRight
                }
            }
        }

        Rectangle {
            width: parent.width
            height: stateCol.implicitHeight + 2 * Theme.space3
            radius: Theme.radius
            color: root.st && (root.st.kind === "failed") ? Theme.dangerSoft
                 : root.st && (root.st.kind === "unauthorized" || root.st.kind === "unconfigured" || root.st.kind === "uncertain") ? Theme.warningSoft
                 : Theme.surfaceRaised
            Column {
                id: stateCol
                anchors.left: parent.left
                anchors.right: parent.right
                anchors.verticalCenter: parent.verticalCenter
                anchors.margins: Theme.space3
                spacing: 4
                Text {
                    width: parent.width
                    text: !root.st ? "" : root.st.kind === "ready" ? "Ready" : root.st.status
                    color: Theme.text
                    font.pixelSize: Theme.fontBody
                    font.weight: Font.DemiBold
                }
                Text {
                    width: parent.width
                    visible: text.length > 0
                    wrapMode: Text.Wrap
                    text: !root.st ? "" : root.asked && root.st.kind === "unauthorized"
                        ? "Waiting for an answer on " + root.host + ". Choose Allow in the notification there."
                        : root.st.kind === "ready" ? "Tap the tile to run it on " + root.host + "." : root.st.detail
                    color: Theme.textMuted
                    font.pixelSize: Theme.fontLabel
                }
            }
        }

        Row {
            anchors.right: parent.right
            spacing: Theme.space2
            PocketButton {
                variant: "ghost"
                text: "Close"
                onActivated: Shell.closeTileDetail()
            }
            PocketButton {
                visible: root.st !== null && root.st.kind === "unauthorized" && Shell.canRequest(root.tile.action)
                variant: "primary"
                icon: "shield"
                enabled: !root.asked
                text: root.asked ? "Asked " + root.host : "Ask " + root.host
                onActivated: Shell.requestAccess(root.tile.action)
            }
            PocketButton {
                visible: root.st !== null && Shell.dock.deckEditable
                         && (root.st.kind === "unconfigured" || root.st.kind === "missing")
                icon: "edit"
                text: "Edit tile"
                onActivated: {
                    const t = root.tile
                    Shell.closeTileDetail()
                    Shell.beginDeckEdit()
                    Shell.editDeckTile(t)
                }
            }
            PocketButton {
                visible: root.st !== null && (root.st.kind === "ready" || root.st.kind === "done"
                                              || root.st.kind === "failed" || root.st.kind === "uncertain")
                         && root.action !== null && root.action.available
                variant: "primary"
                icon: root.st && root.st.kind === "uncertain" ? "refresh" : "play"
                text: root.st && root.st.kind === "uncertain" ? "Retry" : "Run"
                onActivated: {
                    const t = root.tile, k = root.st.kind
                    Shell.closeTileDetail()
                    if (k === "uncertain")
                        Shell.retryDeckTile(t)
                    else
                        Shell.runDeckTile(t)
                }
            }
        }
    }
}
