pragma ComponentBehavior: Bound
import QtQuick
import "../../components"
import "../../state"
import "../../theme"

// Control Deck: pages of tiles from Pocket Core's validated deck configuration
// (Shell.dock.deck, docs/dock-mode/architecture.md#deck-configuration). Tiles only name
// actions; what is available comes from the laptop's advertisement at run time.
Item {
    id: root
    readonly property var dock: Shell.dock
    readonly property var pages: dock.deck.pages || []
    readonly property var page: {
        for (const p of pages)
            if (p.id === Shell.dockNav.page)
                return p
        return pages.length > 0 ? pages[0] : null
    }
    readonly property bool connected: dock.supported && dock.state === "connected"

    // ── Page chips ───────────────────────────────────────────────────────────────────
    Flickable {
        id: chips
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: parent.top
        anchors.margins: Theme.space3
        height: 36
        contentWidth: chipRow.width
        flickableDirection: Flickable.HorizontalFlick
        boundsBehavior: Flickable.StopAtBounds
        clip: true
        Row {
            id: chipRow
            spacing: Theme.space2
            Repeater {
                model: root.pages
                delegate: Rectangle {
                    id: chip
                    required property var modelData
                    readonly property bool selected: root.page !== null && root.page.id === modelData.id
                    height: 36
                    width: chipText.implicitWidth + 2 * Theme.space4
                    radius: height / 2
                    color: chipTap.pressed ? Theme.surfacePressed : selected ? Theme.accent : Theme.surfaceRaised
                    Text {
                        id: chipText
                        anchors.centerIn: parent
                        text: chip.modelData.title
                        color: chip.selected ? Theme.accentText : Theme.text
                        font.pixelSize: Theme.fontLabel
                        font.weight: Font.Medium
                    }
                    TapHandler {
                        id: chipTap
                        gesturePolicy: TapHandler.ReleaseWithinBounds
                        onTapped: Shell.dockNav.selectPage(chip.modelData.id)
                    }
                }
            }
        }
    }
    Text {
        anchors.right: parent.right
        anchors.rightMargin: Theme.space4
        anchors.verticalCenter: chips.verticalCenter
        visible: !root.connected && root.pages.length > 0
        text: root.dock.state === "connecting" ? "Connecting…" : "Not connected"
        color: Theme.warning
        font.pixelSize: Theme.fontCaption
    }

    // ── Tiles ────────────────────────────────────────────────────────────────────────
    Flickable {
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: chips.bottom
        anchors.bottom: parent.bottom
        anchors.margins: Theme.space3
        contentHeight: grid.height
        boundsBehavior: Flickable.StopAtBounds
        clip: true
        Grid {
            id: grid
            width: parent.width
            columns: 4
            spacing: Theme.space2
            readonly property real cellWidth: (width - (columns - 1) * spacing) / columns
            Repeater {
                model: root.page ? root.page.tiles : []
                delegate: DeckTile {
                    required property var modelData
                    width: grid.cellWidth
                    height: 92
                    tile: modelData
                    action: root.dock.actions.length >= 0 ? Shell.dockAction(modelData.action) : null
                    connected: root.connected
                    pending: root.dock.pendingActions[modelData.action] !== undefined
                             && Shell._tileByAction[modelData.action] === modelData.id
                    result: Shell.tileResults[modelData.id] || null
                }
            }
        }
    }
    Text {
        anchors.centerIn: parent
        width: parent.width - 2 * Theme.space5
        visible: root.pages.length === 0
        horizontalAlignment: Text.AlignHCenter
        wrapMode: Text.Wrap
        text: !Shell.coreConnected ? "Pocket Core is not connected."
            : !root.dock.platform ? "This Pocket Core has no Control Deck (update pocketd)."
            : "The deck has no pages."
        color: Theme.textMuted
        font.pixelSize: Theme.fontBody
    }
}
