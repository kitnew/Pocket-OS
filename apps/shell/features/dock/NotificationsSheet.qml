pragma ComponentBehavior: Bound
import QtQuick
import "../../components"
import "../../state"
import "../../theme"

// Notification history (modal side panel): newest first, kept in memory by the shell
// (max 50). Swipe an entry sideways to remove it from the phone; "Mute" drops that app's
// future notifications (a Core preference, persisted). Do Not Disturb stops banners.
Sheet {
    id: root
    readonly property var prefs: Shell.dock.preferences
    readonly property var muted: prefs.muted_apps || []
    open: Shell.dockNav.modal === "notifications"
    edge: "right"
    cardWidth: Math.min(width * 0.62, 480)
    padding: Theme.space4
    onDismissed: Shell.dockNav.closeModal()

    Item {
        id: header
        width: parent.width
        height: Theme.touchTarget
        Text {
            anchors.verticalCenter: parent.verticalCenter
            text: "Notifications"
            color: Theme.text
            font.pixelSize: Theme.fontTitle
            font.weight: Font.DemiBold
        }
        Row {
            anchors.right: parent.right
            anchors.verticalCenter: parent.verticalCenter
            spacing: Theme.space1
            IconButton {
                icon: "moon"
                accessibleName: root.prefs.dnd ? "Turn off Do Not Disturb" : "Do Not Disturb"
                checked: root.prefs.dnd === true
                onActivated: Shell.dock.setPreference("dnd", !root.prefs.dnd)
            }
            IconButton {
                icon: "trash"
                enabled: Shell.dockNotifications.count > 0
                accessibleName: "Clear all"
                onActivated: Shell.clearDockNotifications()
            }
            IconButton { icon: "close"; accessibleName: "Close"; onActivated: Shell.dockNav.closeModal() }
        }
    }
    Text {
        id: sub
        anchors.top: header.bottom
        width: parent.width
        text: (root.prefs.dnd ? "Do Not Disturb · no banners" : "From " + (Shell.dock.hostName || "the laptop"))
              + (root.muted.length > 0 ? " · muted: " + root.muted.join(", ") : "")
        color: Theme.textMuted
        font.pixelSize: Theme.fontCaption
        elide: Text.ElideRight
    }
    Chip {
        id: unmute
        anchors.top: sub.bottom
        anchors.topMargin: Theme.space2
        visible: root.muted.length > 0
        height: visible ? 32 : 0
        interactive: true
        icon: "volume"
        text: "Unmute all"
        onTapped: {
            for (const a of root.muted)
                Shell.dock.setPreference("unmute_app", a)
        }
    }
    ListView {
        id: list
        anchors.top: unmute.bottom
        anchors.topMargin: Theme.space3
        width: parent.width
        height: parent.height - y
        spacing: Theme.space2
        clip: true
        boundsBehavior: Flickable.StopAtBounds
        model: Shell.dockNotifications
        add: Transition { NumberAnimation { property: "opacity"; from: 0; to: 1; duration: Theme.duration } }
        displaced: Transition { NumberAnimation { property: "y"; duration: Theme.duration; easing.type: Theme.easing } }
        delegate: Item {
            id: entry
            required property int index
            required property string nid
            required property string app
            required property string summary
            required property string body
            required property bool redacted
            required property string time
            width: ListView.view.width
            height: 68

            Rectangle {
                id: row
                width: parent.width
                height: parent.height
                x: swipe.active ? swipe.translation.x : 0
                opacity: 1 - Math.min(0.7, Math.abs(x) / width)
                radius: 14
                color: Theme.surfaceRaised
                Behavior on x { enabled: !swipe.active; NumberAnimation { duration: Theme.duration } }
                Column {
                    anchors.left: parent.left
                    anchors.right: mute.left
                    anchors.verticalCenter: parent.verticalCenter
                    anchors.leftMargin: Theme.space3
                    spacing: 2
                    Text {
                        width: parent.width
                        text: entry.app + " · " + entry.time
                        color: Theme.textMuted
                        font.pixelSize: Theme.fontCaption
                        elide: Text.ElideRight
                    }
                    Text {
                        width: parent.width
                        text: entry.summary
                        color: Theme.text
                        font.pixelSize: Theme.fontLabel
                        font.weight: Font.DemiBold
                        elide: Text.ElideRight
                    }
                    Text {
                        width: parent.width
                        text: entry.redacted ? "Hidden while the laptop is locked" : entry.body
                        visible: text.length > 0
                        color: Theme.textMuted
                        font.pixelSize: Theme.fontCaption
                        elide: Text.ElideRight
                    }
                }
                PocketButton {
                    id: mute
                    anchors.right: parent.right
                    anchors.rightMargin: Theme.space2
                    anchors.verticalCenter: parent.verticalCenter
                    compact: true
                    variant: "ghost"
                    text: "Mute"
                    visible: entry.app.length > 0
                    onActivated: Shell.dock.setPreference("mute_app", entry.app)
                }
                DragHandler {
                    id: swipe
                    target: null
                    yAxis.enabled: false
                    onActiveChanged: if (!active && Math.abs(translation.x) > row.width * 0.35)
                                         Shell.removeDockNotification(entry.nid)
                }
            }
        }
        Column {
            anchors.centerIn: parent
            visible: Shell.dockNotifications.count === 0
            spacing: Theme.space2
            Icon { anchors.horizontalCenter: parent.horizontalCenter; name: "bell"; opacity: 0.4; size: Theme.iconLarge }
            Text {
                text: "No notifications"
                color: Theme.textMuted
                font.pixelSize: Theme.fontBody
            }
        }
    }
}
