pragma ComponentBehavior: Bound
import QtQuick
import "../../components"
import "../../state"
import "../../theme"

// Notification history (modal): newest first, kept in memory by the shell (max 50).
// Do Not Disturb and per-app mute are Core preferences (persisted).
Rectangle {
    id: root
    color: Theme.background
    readonly property var prefs: Shell.dock.preferences
    readonly property var muted: prefs.muted_apps || []
    TapHandler {}   // modal: nothing below receives taps

    Item {
        id: header
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: parent.top
        anchors.margins: Theme.space3
        height: Theme.touchTarget
        Text {
            anchors.verticalCenter: parent.verticalCenter
            text: "Laptop notifications"
            color: Theme.text
            font.pixelSize: Theme.fontTitle
            font.weight: Font.DemiBold
        }
        Row {
            anchors.right: parent.right
            spacing: Theme.space2
            PocketButton {
                text: root.prefs.dnd ? "Do Not Disturb: on" : "Do Not Disturb: off"
                primary: root.prefs.dnd === true
                onActivated: Shell.dock.setPreference("dnd", !root.prefs.dnd)
            }
            PocketButton { text: "Clear"; onActivated: Shell.clearDockNotifications() }
            IconButton { icon: "close"; accessibleName: "Close"; onActivated: Shell.dockNav.closeModal() }
        }
    }
    Text {
        id: mutedLine
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: header.bottom
        anchors.leftMargin: Theme.space3
        visible: root.muted.length > 0
        height: visible ? implicitHeight + Theme.space1 : 0
        text: "Muted: " + root.muted.join(", ") + " (tap to unmute all)"
        color: Theme.textMuted
        font.pixelSize: Theme.fontCaption
        elide: Text.ElideRight
        TapHandler {
            onTapped: {
                for (const a of root.muted)
                    Shell.dock.setPreference("unmute_app", a)
            }
        }
    }
    ListView {
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: mutedLine.bottom
        anchors.bottom: parent.bottom
        anchors.margins: Theme.space3
        spacing: Theme.space2
        clip: true
        model: Shell.dockNotifications
        delegate: Rectangle {
            id: entry
            required property string app
            required property string summary
            required property string body
            required property bool redacted
            required property string time
            width: ListView.view.width
            height: 56
            radius: Theme.radiusSmall
            color: Theme.surfaceRaised
            Column {
                anchors.left: parent.left
                anchors.right: mute.left
                anchors.verticalCenter: parent.verticalCenter
                anchors.leftMargin: Theme.space3
                spacing: 1
                Text {
                    width: parent.width
                    text: entry.time + " · " + entry.app + " · " + entry.summary
                    color: Theme.text
                    font.pixelSize: Theme.fontLabel
                    elide: Text.ElideRight
                }
                Text {
                    width: parent.width
                    text: entry.redacted ? "Hidden while the laptop is locked" : entry.body
                    color: Theme.textMuted
                    font.pixelSize: Theme.fontCaption
                    elide: Text.ElideRight
                }
            }
            PocketButton {
                id: mute
                anchors.right: parent.right
                anchors.rightMargin: Theme.space1
                anchors.verticalCenter: parent.verticalCenter
                text: "Mute app"
                visible: entry.app.length > 0
                onActivated: Shell.dock.setPreference("mute_app", entry.app)
            }
        }
        Text {
            anchors.centerIn: parent
            visible: Shell.dockNotifications.count === 0
            text: "No notifications"
            color: Theme.textMuted
            font.pixelSize: Theme.fontBody
        }
    }
}
