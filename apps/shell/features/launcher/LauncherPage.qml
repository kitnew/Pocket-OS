pragma ComponentBehavior: Bound
import QtQuick
import Pocket.Core
import "../../components"
import "../../state"
import "../../theme"

// Application launcher: all installed apps (Pocket Core catalog) in a scrolling grid,
// with search. Tapping an app opens it, or switches to its window if already running.
Item {
    id: root

    function reset() {
        search.text = ""
        search.focus = false
        grid.positionViewAtBeginning()
    }

    // Search field (plain TextInput: no Qt Quick Controls dependency). Typing needs the
    // on-screen keyboard, which squeekboard shows when this field takes focus.
    Rectangle {
        id: searchBox
        anchors.top: parent.top
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.margins: Theme.space4
        height: Theme.touchTarget
        radius: height / 2
        color: Theme.surfaceRaised
        border.color: search.activeFocus ? Theme.accent : "transparent"
        border.width: 1

        Icon {
            id: searchIcon
            anchors.left: parent.left
            anchors.leftMargin: Theme.space3
            anchors.verticalCenter: parent.verticalCenter
            name: "search"
            size: Theme.iconSmall + 2
            opacity: 0.7
        }
        TextInput {
            id: search
            anchors.left: searchIcon.right
            anchors.right: clear.left
            anchors.leftMargin: Theme.space2
            anchors.verticalCenter: parent.verticalCenter
            color: Theme.text
            font.pixelSize: Theme.fontBody
            clip: true
            inputMethodHints: Qt.ImhNoPredictiveText
            onTextChanged: filter.query = text
            onActiveFocusChanged: Shell.diag("launcher.search", { focus: activeFocus })
            Text {
                anchors.verticalCenter: parent.verticalCenter
                visible: !search.text && !search.activeFocus
                text: "Search apps"
                color: Theme.textMuted
                font.pixelSize: Theme.fontBody
            }
        }
        IconButton {
            id: clear
            anchors.right: parent.right
            anchors.verticalCenter: parent.verticalCenter
            visible: search.text.length > 0
            icon: "close"
            iconSize: Theme.iconSmall
            accessibleName: "Clear search"
            onActivated: search.text = ""
        }
        TapHandler { onTapped: search.forceActiveFocus() }
    }

    AppFilter {
        id: filter
        sourceModel: Shell.apps
    }

    GridView {
        id: grid
        anchors.top: searchBox.bottom
        anchors.topMargin: Theme.space3
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.bottom: parent.bottom
        anchors.leftMargin: Theme.space2
        anchors.rightMargin: Theme.space2
        clip: true
        model: filter
        readonly property int columns: Math.max(3, Math.floor(width / 84))
        cellWidth: width / columns
        cellHeight: Theme.appIcon + 48
        boundsBehavior: Flickable.StopAtBounds
        delegate: AppTile {
            id: tile
            required property var model
            width: grid.cellWidth
            height: grid.cellHeight
            name: tile.model.name
            iconSource: tile.model.iconSource
            running: Shell.windowCount >= 0 && Shell.windowFor(tile.model.appId) !== null
            onActivated: {
                search.focus = false
                Shell.openApp(tile.model.appId)
            }
        }
    }

    Text {
        anchors.centerIn: grid
        width: grid.width - 2 * Theme.space5
        horizontalAlignment: Text.AlignHCenter
        wrapMode: Text.Wrap
        visible: filter.count === 0
        text: !Shell.apps.loaded ? (Shell.coreConnected ? "Loading applications…" : "Waiting for Pocket Core…")
            : Shell.apps.count === 0 ? "No applications installed"
            : "No apps match “" + filter.query + "”"
        color: Theme.textMuted
        font.pixelSize: Theme.fontBody
    }
    Text {
        anchors.bottom: parent.bottom
        anchors.horizontalCenter: parent.horizontalCenter
        anchors.bottomMargin: Theme.space2
        visible: Shell.apps.stale
        text: "Pocket Core disconnected — list may be outdated"
        color: Theme.warning
        font.pixelSize: Theme.fontCaption
    }
}
