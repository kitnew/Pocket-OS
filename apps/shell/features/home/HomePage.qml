pragma ComponentBehavior: Bound
import QtQuick
import "../../components"
import "../../state"
import "../../theme"

// Home page: clock, running windows (tap to switch), app shortcuts, "All apps".
Item {
    id: root
    readonly property bool landscape: width > height

    // Shortcuts: favorites that are installed, then the rest alphabetically.
    readonly property int maxShortcuts: landscape ? 8 : 12
    readonly property var shortcuts: {
        const out = []
        const seen = {}
        const n = Shell.apps.count // dependency: re-evaluate when the catalog changes
        for (const id of Shell.favoriteIds) {
            const e = Shell.apps.get(id)
            if (e.appId && !seen[id]) { out.push(e); seen[id] = true }
        }
        for (let i = 0; i < n && out.length < maxShortcuts; ++i) {
            const e = Shell.apps.get(Shell.apps.idAt(i))
            if (e.appId && !seen[e.appId]) { out.push(e); seen[e.appId] = true }
        }
        return out.slice(0, maxShortcuts)
    }

    Grid {
        anchors.fill: parent
        anchors.margins: Theme.space5
        columns: root.landscape ? 2 : 1
        columnSpacing: Theme.space6
        rowSpacing: Theme.space5

        // Clock and identity
        Column {
            id: header
            width: root.landscape ? (parent.width - Theme.space6) * 0.4 : parent.width
            spacing: Theme.space1
            Text {
                text: Qt.formatTime(Shell.now, "HH:mm")
                color: Theme.text
                font.pixelSize: Theme.fontDisplay
                font.weight: Font.Light
            }
            Text {
                text: Qt.formatDate(Shell.now, "dddd, d MMMM")
                color: Theme.textMuted
                font.pixelSize: Theme.fontBody
            }
            Row {
                topPadding: Theme.space2
                spacing: Theme.space2
                Text {
                    text: "Pocket OS"
                    color: Theme.textMuted
                    font.pixelSize: Theme.fontLabel
                    font.weight: Font.DemiBold
                }
                Text {
                    visible: Shell.core.mode === "dock"
                    text: "· Dock mode"
                    color: Theme.accent
                    font.pixelSize: Theme.fontLabel
                }
                Text {
                    visible: !Shell.coreConnected
                    text: "· Core " + Shell.core.statusText.toLowerCase()
                    color: Theme.warning
                    font.pixelSize: Theme.fontLabel
                }
            }
        }

        Column {
            width: root.landscape ? (parent.width - Theme.space6) * 0.6 : parent.width
            spacing: Theme.space4

            // Running windows
            Column {
                width: parent.width
                spacing: Theme.space2
                visible: Shell.windowCount > 0
                Text {
                    text: "Running"
                    color: Theme.textMuted
                    font.pixelSize: Theme.fontLabel
                    font.weight: Font.DemiBold
                }
                Flickable {
                    width: parent.width
                    height: 44
                    contentWidth: running.width
                    flickableDirection: Flickable.HorizontalFlick
                    clip: true
                    Row {
                        id: running
                        spacing: Theme.space2
                        Repeater {
                            model: Shell.windowList
                            delegate: Rectangle {
                                id: chipRoot
                                required property var modelData
                                readonly property var entry: Shell.apps.get(Shell.apps.idForWindow(chipRoot.modelData.appId))
                                height: 44
                                width: Math.min(200, chip.implicitWidth + Theme.space4)
                                radius: height / 2
                                color: chipTap.pressed ? Theme.surfacePressed : Theme.surfaceRaised
                                Row {
                                    id: chip
                                    anchors.verticalCenter: parent.verticalCenter
                                    x: Theme.space2
                                    spacing: Theme.space2
                                    AppIcon {
                                        anchors.verticalCenter: parent.verticalCenter
                                        size: 28
                                        source: chipRoot.entry.iconSource || ""
                                        name: chipRoot.entry.name || chipRoot.modelData.appId
                                    }
                                    Text {
                                        anchors.verticalCenter: parent.verticalCenter
                                        width: Math.min(implicitWidth, 140)
                                        text: chipRoot.entry.name || chipRoot.modelData.title || chipRoot.modelData.appId
                                        color: Theme.text
                                        font.pixelSize: Theme.fontLabel
                                        elide: Text.ElideRight
                                    }
                                }
                                TapHandler {
                                    id: chipTap
                                    gesturePolicy: TapHandler.ReleaseWithinBounds
                                    onTapped: Shell.activateWindow(chipRoot.modelData)
                                }
                            }
                        }
                    }
                }
            }

            // Shortcuts
            Text {
                text: "Apps"
                color: Theme.textMuted
                font.pixelSize: Theme.fontLabel
                font.weight: Font.DemiBold
            }
            Grid {
                id: grid
                width: parent.width
                columns: Math.max(3, Math.min(6, Math.floor(width / 80)))
                readonly property real cell: width / columns
                Repeater {
                    model: root.shortcuts
                    delegate: AppTile {
                        id: shortcut
                        required property var modelData
                        width: grid.cell
                        name: shortcut.modelData.name
                        iconSource: shortcut.modelData.iconSource
                        running: Shell.windowCount >= 0 && Shell.windowFor(shortcut.modelData.appId) !== null
                        onActivated: Shell.openApp(shortcut.modelData.appId)
                    }
                }
            }
            Text {
                visible: root.shortcuts.length === 0
                width: parent.width
                wrapMode: Text.Wrap
                text: !Shell.apps.loaded ? (Shell.coreConnected ? "Loading applications…" : "Waiting for Pocket Core…")
                                          : "No applications installed"
                color: Theme.textMuted
                font.pixelSize: Theme.fontBody
            }
            PocketButton {
                text: "All apps"
                primary: true
                onActivated: Shell.showPage("apps")
            }
        }
    }
}
