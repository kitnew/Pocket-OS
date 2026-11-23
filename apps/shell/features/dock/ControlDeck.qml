pragma ComponentBehavior: Bound
import QtQuick
import "../../components"
import "../../state"
import "../../theme"

// Control Deck: pages of tiles from Pocket Core's validated deck configuration
// (Shell.shownDeck: the saved deck, or the working copy while editing).
//
//   ┌────────────────────────────────────────────────────────────┐
//   │ [General] Development  Media              ✎    ▭ 100% 23:09│ header: tabs, edit, status
//   │ ┌────────┐ ┌────────┐ ┌────────┐ ┌────────┐                │
//   │ │ ▣      │ │ ▣    🔒│ │ ▣      │ │ ▣      │                │ 4 × 2 tiles per screen;
//   │ │ Lock   │ │Overview│ │Desktop │ │ Ping   │                │ swipe sideways between
//   │ └────────┘ └────────┘ └────────┘ └────────┘                │ pages (or tap a tab);
//   │ ┌────────┐ ┌────────┐                                      │ more than 8 tiles scroll
//   │ └────────┘ └────────┘               • ○ ○                  │ vertically within a page
//   └────────────────────────────────────────────────────────────┘
//
// Tiles only name actions; what can run comes from the laptop's advertisement.
// Edit mode (✎): tap a tile to edit it, ＋ to add one, tap the current tab to rename,
// move or remove the page; Done saves through Core (validated), Cancel discards.
Item {
    id: root
    readonly property var dock: Shell.dock
    readonly property var pages: Shell.shownDeck.pages || []
    readonly property bool editing: Shell.deckEditing
    readonly property int pageIndex: {
        for (let i = 0; i < pages.length; ++i)
            if (pages[i].id === Shell.dockNav.page)
                return i
        return 0
    }

    // ── Header ───────────────────────────────────────────────────────────────────────
    Item {
        id: header
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: parent.top
        anchors.topMargin: Theme.space3
        anchors.leftMargin: Theme.space3
        anchors.rightMargin: Theme.gutter
        height: 44

        Flickable {
            id: tabs
            anchors.left: parent.left
            anchors.right: trailing.left
            anchors.rightMargin: Theme.space3
            height: parent.height
            contentWidth: tabRow.width
            flickableDirection: Flickable.HorizontalFlick
            boundsBehavior: Flickable.StopAtBounds
            clip: true
            Row {
                id: tabRow
                height: parent.height
                spacing: Theme.space1
                Repeater {
                    model: root.pages
                    delegate: Pressable {
                        id: tab
                        required property var modelData
                        required property int index
                        readonly property bool selected: index === root.pageIndex
                        height: 40
                        anchors.verticalCenter: parent.verticalCenter
                        width: tabText.implicitWidth + 2 * Theme.space4
                        pressScale: 0.96
                        Accessible.role: Accessible.PageTab
                        Accessible.name: modelData.title
                        onTapped: {
                            if (root.editing && selected)
                                Shell.editDeckPage(modelData.id)
                            else
                                Shell.dockNav.selectPage(modelData.id)
                        }
                        Rectangle {
                            anchors.fill: parent
                            radius: height / 2
                            color: tab.pressed ? Theme.surfacePressed : tab.selected ? Theme.surfaceHigh : "transparent"
                            Behavior on color { ColorAnimation { duration: Theme.durationFast } }
                        }
                        Row {
                            anchors.centerIn: parent
                            spacing: Theme.space1
                            Text {
                                id: tabText
                                text: tab.modelData.title
                                color: tab.selected ? Theme.text : Theme.textMuted
                                font.pixelSize: Theme.fontBody
                                font.weight: tab.selected ? Font.DemiBold : Font.Normal
                            }
                            Icon {
                                anchors.verticalCenter: parent.verticalCenter
                                visible: root.editing && tab.selected
                                name: "edit"
                                size: 14
                            }
                        }
                    }
                }
                IconButton {
                    anchors.verticalCenter: parent.verticalCenter
                    visible: root.editing && root.pages.length < 12
                    icon: "plus"
                    iconSize: Theme.iconSmall + 2
                    accessibleName: "Add page"
                    onActivated: Shell.addDeckPage()
                }
            }
        }

        Row {
            id: trailing
            anchors.right: parent.right
            anchors.verticalCenter: parent.verticalCenter
            spacing: Theme.space3
            IconButton {
                anchors.verticalCenter: parent.verticalCenter
                visible: !root.editing && root.dock.deckEditable && root.pages.length > 0
                icon: "edit"
                iconSize: Theme.iconSmall + 2
                accessibleName: "Edit deck"
                onActivated: Shell.beginDeckEdit()
            }
            PocketButton {
                anchors.verticalCenter: parent.verticalCenter
                visible: root.editing
                compact: true
                variant: "ghost"
                text: "Cancel"
                onActivated: Shell.cancelDeckEdit()
            }
            PocketButton {
                anchors.verticalCenter: parent.verticalCenter
                visible: root.editing
                compact: true
                variant: "primary"
                icon: "check"
                text: Shell.deckSaving ? "Saving…" : "Done"
                enabled: !Shell.deckSaving
                onActivated: Shell.saveDeckEdit()
            }
            DockStatus {
                anchors.verticalCenter: parent.verticalCenter
                visible: !root.editing
                showTime: true
            }
        }
    }

    // ── Offline / problem banner ─────────────────────────────────────────────────────
    Rectangle {
        id: banner
        readonly property bool show: !Shell.dockConnected && root.pages.length > 0 && !root.editing
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: header.bottom
        anchors.topMargin: show ? Theme.space2 : 0
        anchors.leftMargin: Theme.space3
        anchors.rightMargin: Theme.gutter
        height: show ? 40 : 0
        visible: show
        radius: 12
        color: Theme.warningSoft
        Row {
            anchors.left: parent.left
            anchors.leftMargin: Theme.space3
            anchors.verticalCenter: parent.verticalCenter
            spacing: Theme.space2
            Icon { anchors.verticalCenter: parent.verticalCenter; name: "link-off"; size: Theme.iconSmall }
            Text {
                anchors.verticalCenter: parent.verticalCenter
                text: !Shell.coreConnected ? "Pocket Core is not running"
                    : !root.dock.configured ? "No laptop paired"
                    : root.dock.state === "connecting" ? "Connecting to " + root.dock.hostName + "…"
                    : root.dock.retryScheduled && root.dock.retrySeconds > 0
                      ? root.dock.hostName + " is offline · retrying in " + root.dock.retrySeconds + " s"
                    : root.dock.hostName + " is offline"
                color: Theme.text
                font.pixelSize: Theme.fontLabel
            }
        }
        PocketButton {
            anchors.right: parent.right
            anchors.rightMargin: 4
            anchors.verticalCenter: parent.verticalCenter
            height: 32
            compact: true
            visible: root.dock.configured && root.dock.state !== "connecting"
            text: root.dock.state === "error" ? "Retry now" : "Connect"
            onActivated: Shell.connectDock()
        }
    }

    // ── Pages (swipe sideways) ───────────────────────────────────────────────────────
    ListView {
        id: pager
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: banner.bottom
        anchors.bottom: dots.top
        anchors.topMargin: Theme.space3
        orientation: ListView.Horizontal
        snapMode: ListView.SnapOneItem
        highlightRangeMode: ListView.StrictlyEnforceRange
        preferredHighlightBegin: 0
        preferredHighlightEnd: width
        highlightMoveDuration: Theme.durationSlow
        boundsBehavior: Flickable.StopAtBounds
        // A deliberate sideways swipe only: short drags stay taps.
        flickDeceleration: 4000
        maximumFlickVelocity: 4000
        clip: true
        model: root.pages
        // Tabs/IPC → pager (a swipe sets currentIndex itself, so this is not a binding).
        Component.onCompleted: currentIndex = root.pageIndex
        Connections {
            target: root
            function onPageIndexChanged() {
                if (pager.currentIndex !== root.pageIndex)
                    pager.currentIndex = root.pageIndex
            }
        }
        onCurrentIndexChanged: {
            if (currentIndex >= 0 && currentIndex < root.pages.length
                    && root.pages[currentIndex].id !== Shell.dockNav.page)
                Shell.dockNav.selectPage(root.pages[currentIndex].id)
        }

        delegate: Flickable {
            id: page
            required property var modelData
            required property int index
            width: pager.width
            height: pager.height
            contentHeight: grid.height
            flickableDirection: Flickable.VerticalFlick
            boundsBehavior: Flickable.StopAtBounds
            interactive: contentHeight > height
            clip: true

            Grid {
                id: grid
                x: Theme.space3
                width: page.width - Theme.space3 - Theme.gutter
                columns: 4
                spacing: Theme.space2
                readonly property real cellWidth: (width - (columns - 1) * spacing) / columns
                // Two rows fill the page; never smaller than a comfortable touch target.
                readonly property real cellHeight: Math.max(96, Math.min(150, (page.height - spacing) / 2))
                Repeater {
                    model: page.modelData.tiles
                    delegate: DeckTile {
                        required property var modelData
                        width: grid.cellWidth
                        height: grid.cellHeight
                        tile: modelData
                        editing: root.editing
                    }
                }
                // Edit mode: add a tile.
                Pressable {
                    visible: root.editing && page.modelData.tiles.length < 24
                    width: grid.cellWidth
                    height: grid.cellHeight
                    onTapped: Shell.addDeckTile(page.modelData.id)
                    Accessible.role: Accessible.Button
                    Accessible.name: "Add tile"
                    Rectangle {
                        anchors.fill: parent
                        radius: Theme.radiusLarge - 4
                        color: parent.pressed ? Theme.surfacePressed : "transparent"
                        border.width: 1.5
                        border.color: Theme.outlineStrong
                    }
                    Column {
                        anchors.centerIn: parent
                        spacing: Theme.space1
                        Icon { anchors.horizontalCenter: parent.horizontalCenter; name: "plus" }
                        Text {
                            text: "Add tile"
                            color: Theme.textMuted
                            font.pixelSize: Theme.fontLabel
                        }
                    }
                }
            }
            // An empty page.
            Text {
                anchors.centerIn: parent
                visible: page.modelData.tiles.length === 0 && !root.editing
                text: root.dock.deckEditable ? "This page is empty. Tap ✎ to add tiles." : "This page is empty."
                color: Theme.textMuted
                font.pixelSize: Theme.fontBody
            }
        }
    }

    // Page indicator (also hints that pages swipe).
    Row {
        id: dots
        anchors.horizontalCenter: pager.horizontalCenter
        anchors.bottom: parent.bottom
        anchors.bottomMargin: Theme.space2
        height: root.pages.length > 1 ? 8 : 0
        spacing: 6
        Repeater {
            model: root.pages.length > 1 ? root.pages.length : 0
            delegate: Rectangle {
                required property int index
                anchors.verticalCenter: parent.verticalCenter
                width: index === root.pageIndex ? 16 : 6
                height: 6
                radius: 3
                color: index === root.pageIndex ? Theme.text : Theme.textDisabled
                Behavior on width { NumberAnimation { duration: Theme.duration } }
            }
        }
    }

    // No deck at all.
    Text {
        anchors.centerIn: parent
        width: parent.width - 2 * Theme.space5
        visible: root.pages.length === 0
        horizontalAlignment: Text.AlignHCenter
        wrapMode: Text.Wrap
        text: !Shell.coreConnected ? "Pocket Core is not running."
            : !root.dock.platform ? "This Pocket Core has no Control Deck (update pocketd)."
            : "The deck has no pages."
        color: Theme.textMuted
        font.pixelSize: Theme.fontBody
    }
}
