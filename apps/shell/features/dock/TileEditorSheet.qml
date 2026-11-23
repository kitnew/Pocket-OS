pragma ComponentBehavior: Bound
import QtQuick
import "../../components"
import "../../state"
import "../../theme"

// Deck editor sheet (edit mode, issue #17). Works on Shell.deckDraft; nothing is stored
// until Done in the Deck header (Core validates and persists it).
//
// Tile:  label · icon · what it does (the laptop's actions and their closed lists: apps,
//        places, shortcuts, workflows) · move within/between pages · remove.
//        Actions the laptop has not allowed can be placed (marked); placing never grants.
// Page:  title · move · remove · reset the whole deck to the built-in one.
Sheet {
    id: root
    readonly property var target: Shell.deckEditTarget
    readonly property bool tileMode: target !== null && target.kind === "tile"
    readonly property var tile: tileMode ? Shell.draftTile(target.page, target.index) : null
    readonly property var page: target !== null ? Shell.draftPage(target.page) : null
    readonly property var options: Shell.dock.actions.length >= 0 ? Shell.tileOptions() : []
    readonly property string host: Shell.dock.hostName || "the laptop"
    property bool confirmReset: false

    open: target !== null && Shell.deckEditing
    cardWidth: Math.min(width - 2 * Theme.space3, 740)
    cardHeight: tileMode ? height - 2 * Theme.space3 : -1
    padding: Theme.space4
    onDismissed: root.finish()
    onTargetChanged: confirmReset = false

    function finish() {
        commitText()
        labelInput.clearFocus()
        titleInput.clearFocus()
        Shell.osk.setVisible(false)
        Shell.closeDeckEditor()
    }
    function commitText() {
        if (tileMode && tile !== null && labelInput.text !== tile.label)
            Shell.deckOp("updateTile", target.page, target.index, { label: labelInput.text })
        if (!tileMode && page !== null && titleInput.text !== page.title)
            Shell.deckOp("renamePage", target.page, titleInput.text)
    }
    function optionMatches(o) {
        if (!tile || o.action !== tile.action)
            return false
        for (const k in o.params)
            if (k !== "title" && k !== "body" && (tile.params || {})[k] !== o.params[k])
                return false
        return true
    }
    function choose(o) {
        if (!tileMode)
            return
        if (target.index < 0) {
            Shell.deckOp("addTile", target.page, o, o.label)
            const p = Shell.draftPage(target.page)
            Shell.deckEditTarget = { kind: "tile", page: target.page, index: p.tiles.length - 1 }
        } else {
            // Keep a label the user typed; follow the option's name otherwise.
            const custom = tile && tile.label !== "" && !options.some(x => x.label === tile.label)
            Shell.deckOp("updateTile", target.page, target.index,
                         { option: o, icon: o.icon, label: custom ? undefined : o.label })
        }
    }

    // ── Header ───────────────────────────────────────────────────────────────────────
    Item {
        id: head
        width: parent.width
        height: Theme.touchTarget
        Text {
            anchors.verticalCenter: parent.verticalCenter
            text: !root.tileMode ? "Page" : root.target && root.target.index < 0 ? "New tile" : "Edit tile"
            color: Theme.text
            font.pixelSize: Theme.fontTitle
            font.weight: Font.DemiBold
        }
        Row {
            anchors.right: parent.right
            anchors.verticalCenter: parent.verticalCenter
            spacing: Theme.space2
            PocketButton {
                visible: root.tileMode && root.tile !== null
                compact: true
                variant: "danger"
                icon: "trash"
                text: "Remove"
                onActivated: {
                    Shell.deckOp("removeTile", root.target.page, root.target.index)
                    root.finish()
                }
            }
            PocketButton {
                visible: !root.tileMode && Shell.deckDraft !== null && Shell.deckDraft.pages.length > 1
                compact: true
                variant: "danger"
                icon: "trash"
                text: "Remove page"
                onActivated: {
                    const id = root.target.page
                    root.finish()
                    Shell.deckOp("removePage", id)
                    Shell.dockNav.selectPage(Shell.deckDraft.pages[0].id)
                }
            }
            PocketButton {
                compact: true
                variant: "primary"
                text: "Done"
                onActivated: root.finish()
            }
        }
    }

    // ── Tile ─────────────────────────────────────────────────────────────────────────
    Row {
        anchors.top: head.bottom
        anchors.topMargin: Theme.space3
        width: parent.width
        height: parent.height - head.height - Theme.space3
        visible: root.tileMode
        spacing: Theme.space4

        Column {
            id: side
            width: Math.min(270, parent.width * 0.4)
            height: parent.height
            spacing: Theme.space3
            enabled: root.tile !== null
            opacity: enabled ? 1 : 0.4

            Text { text: "Label"; color: Theme.textMuted; font.pixelSize: Theme.fontCaption }
            TextBox {
                id: labelInput
                width: parent.width
                maximumLength: 40
                placeholder: "Tile name"
                source: root.tile ? root.tile.label : ""
                onAccepted: root.commitText()
            }
            Text { text: "Icon"; color: Theme.textMuted; font.pixelSize: Theme.fontCaption }
            GridView {
                id: icons
                width: parent.width
                height: Math.max(48, parent.height - y - moveRow.height - Theme.space3)
                cellWidth: width / Math.floor(width / 44)
                cellHeight: 44
                clip: true
                boundsBehavior: Flickable.StopAtBounds
                model: Theme.tileIcons
                delegate: Pressable {
                    id: ic
                    required property string modelData
                    width: icons.cellWidth
                    height: icons.cellHeight
                    pressScale: 0.9
                    readonly property bool selected: root.tile !== null && root.tile.icon === modelData
                    onTapped: Shell.deckOp("updateTile", root.target.page, root.target.index, { icon: modelData })
                    Rectangle {
                        anchors.centerIn: parent
                        width: 38; height: 38; radius: 11
                        color: ic.selected ? Theme.accent : ic.pressed ? Theme.surfacePressed : Theme.surfaceRaised
                    }
                    Icon { anchors.centerIn: parent; name: ic.modelData; size: 18 }
                }
            }
            Flow {
                id: moveRow
                width: parent.width
                spacing: Theme.space2
                PocketButton {
                    compact: true
                    icon: "arrow-left"
                    text: ""
                    width: 48
                    enabled: root.tile !== null && root.target.index > 0
                    Accessible.name: "Move earlier"
                    onActivated: {
                        Shell.deckOp("moveTile", root.target.page, root.target.index, -1)
                        Shell.deckEditTarget = { kind: "tile", page: root.target.page, index: root.target.index - 1 }
                    }
                }
                PocketButton {
                    compact: true
                    icon: "arrow-right"
                    text: ""
                    width: 48
                    enabled: root.tile !== null && root.page !== null && root.target.index < root.page.tiles.length - 1
                    Accessible.name: "Move later"
                    onActivated: {
                        Shell.deckOp("moveTile", root.target.page, root.target.index, 1)
                        Shell.deckEditTarget = { kind: "tile", page: root.target.page, index: root.target.index + 1 }
                    }
                }
                Repeater {
                    // Move to another page (at most three shown; the rest by moving pages).
                    model: Shell.deckDraft ? Shell.deckDraft.pages.filter(p => root.target && p.id !== root.target.page).slice(0, 3) : []
                    delegate: Chip {
                        required property var modelData
                        height: 40
                        interactive: root.tile !== null
                        icon: "arrow-right"
                        text: modelData.title
                        onTapped: {
                            const to = modelData.id
                            Shell.deckOp("moveTileToPage", root.target.page, root.target.index, to)
                            root.finish()
                            Shell.dockNav.selectPage(to)
                        }
                    }
                }
            }
        }

        // What the tile does: the laptop's actions.
        Item {
            width: parent.width - side.width - parent.spacing
            height: parent.height
            Text {
                id: actionsTitle
                text: "Action on " + root.host
                color: Theme.textMuted
                font.pixelSize: Theme.fontCaption
            }
            ListView {
                id: list
                anchors.top: actionsTitle.bottom
                anchors.topMargin: Theme.space2
                anchors.left: parent.left
                anchors.right: parent.right
                anchors.bottom: parent.bottom
                clip: true
                spacing: 4
                boundsBehavior: Flickable.StopAtBounds
                model: root.options
                // Show the tile's current action when the editor opens.
                onCountChanged: Qt.callLater(scrollToCurrent)
                Connections {
                    target: root
                    function onTargetChanged() { Qt.callLater(list.scrollToCurrent) }
                }
                function scrollToCurrent() {
                    for (let i = 0; i < root.options.length; ++i)
                        if (root.optionMatches(root.options[i])) {
                            list.positionViewAtIndex(i, ListView.Center)
                            return
                        }
                }
                delegate: Pressable {
                    id: opt
                    required property var modelData
                    readonly property bool selected: root.optionMatches(modelData)
                    width: list.width
                    height: 48
                    pressScale: 0.98
                    onTapped: root.choose(modelData)
                    Rectangle {
                        anchors.fill: parent
                        radius: 12
                        color: opt.selected ? Theme.accentSoft : opt.pressed ? Theme.surfacePressed : Theme.surfaceRaised
                        border.width: opt.selected ? 1 : 0
                        border.color: Theme.accent
                    }
                    Icon {
                        x: Theme.space3
                        anchors.verticalCenter: parent.verticalCenter
                        name: opt.modelData.icon
                        size: 18
                    }
                    Column {
                        x: Theme.space3 + 18 + Theme.space3
                        width: parent.width - x - tag.width - Theme.space3 * 2
                        anchors.verticalCenter: parent.verticalCenter
                        Text {
                            width: parent.width
                            text: opt.modelData.label
                            color: Theme.text
                            font.pixelSize: Theme.fontBody
                            elide: Text.ElideRight
                        }
                        Text {
                            width: parent.width
                            text: opt.modelData.group
                            color: Theme.textDisabled
                            font.pixelSize: Theme.fontCaption
                        }
                    }
                    Text {
                        id: tag
                        anchors.right: parent.right
                        anchors.rightMargin: Theme.space3
                        anchors.verticalCenter: parent.verticalCenter
                        text: opt.modelData.available ? "" : opt.modelData.privilege === "elevated" ? "needs permission" : "unavailable now"
                        color: Theme.textMuted
                        font.pixelSize: Theme.fontCaption
                    }
                }
                Text {
                    anchors.centerIn: parent
                    width: parent.width - 2 * Theme.space4
                    horizontalAlignment: Text.AlignHCenter
                    wrapMode: Text.Wrap
                    visible: list.count === 0
                    text: Shell.dockConnected ? root.host + " offers no actions."
                                              : "Connect to " + root.host + " to choose what this tile does. Label, icon and position can be changed now."
                    color: Theme.textMuted
                    font.pixelSize: Theme.fontLabel
                }
            }
        }
    }

    // ── Page ─────────────────────────────────────────────────────────────────────────
    Column {
        anchors.top: head.bottom
        anchors.topMargin: Theme.space3
        width: parent.width
        visible: !root.tileMode
        spacing: Theme.space3
        Text { text: "Title"; color: Theme.textMuted; font.pixelSize: Theme.fontCaption }
        TextBox {
            id: titleInput
            width: Math.min(parent.width, 360)
            maximumLength: 32
            placeholder: "Page title"
            source: root.page ? root.page.title : ""
            onAccepted: root.commitText()
        }
        Row {
            spacing: Theme.space2
            PocketButton {
                compact: true
                icon: "arrow-left"
                text: "Move left"
                enabled: Shell.deckDraft !== null && root.page !== null && Shell.deckDraft.pages.indexOf(root.page) > 0
                onActivated: Shell.deckOp("movePage", root.target.page, -1)
            }
            PocketButton {
                compact: true
                icon: "arrow-right"
                text: "Move right"
                enabled: Shell.deckDraft !== null && root.page !== null
                         && Shell.deckDraft.pages.indexOf(root.page) < Shell.deckDraft.pages.length - 1
                onActivated: Shell.deckOp("movePage", root.target.page, 1)
            }
        }
        Item { width: 1; height: Theme.space2 }
        Row {
            spacing: Theme.space3
            PocketButton {
                compact: true
                variant: root.confirmReset ? "danger" : "ghost"
                icon: "refresh"
                text: root.confirmReset ? "Tap again: reset every page" : "Reset deck to default"
                onActivated: {
                    if (!root.confirmReset) {
                        root.confirmReset = true
                        return
                    }
                    Shell.closeDeckEditor()
                    Shell.resetDeck()
                }
            }
        }
    }
}
