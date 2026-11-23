import QtQuick
import "../../components"
import "../../state"
import "../../theme"

// Laptop notifications received in this shell session: count and the latest one. Tap
// opens the history (a modal sheet).
Rectangle {
    id: root
    readonly property var latest: Shell.dockNotifications.count > 0 ? Shell.dockNotifications.get(0) : null

    implicitHeight: 56
    radius: Theme.radius
    color: tap.pressed ? Theme.surfacePressed : Theme.surfaceRaised

    Row {
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.leftMargin: Theme.space3
        anchors.rightMargin: Theme.space3
        anchors.verticalCenter: parent.verticalCenter
        spacing: Theme.space2
        Icon { anchors.verticalCenter: parent.verticalCenter; name: "bell"; size: Theme.icon }
        Column {
            anchors.verticalCenter: parent.verticalCenter
            width: parent.width - Theme.icon - Theme.space2
            spacing: 1
            Text {
                width: parent.width
                text: Shell.dockNotifications.count + (Shell.dockNotifications.count === 1 ? " notification" : " notifications")
                      + (Shell.dock.preferences.dnd ? " · Do Not Disturb" : "")
                color: Theme.text
                font.pixelSize: Theme.fontLabel
                font.weight: Font.DemiBold
                elide: Text.ElideRight
            }
            Text {
                width: parent.width
                text: root.latest ? root.latest.app + ": " + root.latest.summary : ""
                color: Theme.textMuted
                font.pixelSize: Theme.fontCaption
                elide: Text.ElideRight
            }
        }
    }
    TapHandler {
        id: tap
        gesturePolicy: TapHandler.ReleaseWithinBounds
        onTapped: Shell.dockNav.openModal("notifications")
    }
}
