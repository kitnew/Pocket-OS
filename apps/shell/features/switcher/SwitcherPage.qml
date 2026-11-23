pragma ComponentBehavior: Bound
import QtQuick
import "../../components"
import "../../state"
import "../../theme"

// Open windows (compositor-owned, via Shell.windows): tap to switch, × to close.
// Closing asks the application to close (it may ask to save or refuse).
Item {
    id: root

    Text {
        id: title
        anchors.top: parent.top
        anchors.left: parent.left
        anchors.margins: Theme.space5
        text: "Open windows"
        color: Theme.text
        font.pixelSize: Theme.fontTitle
        font.weight: Font.DemiBold
    }

    ListView {
        id: list
        anchors.top: title.bottom
        anchors.topMargin: Theme.space4
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.bottom: parent.bottom
        anchors.leftMargin: Theme.space4
        anchors.rightMargin: Theme.space4
        spacing: Theme.space2
        clip: true
        model: Shell.windowList
        boundsBehavior: Flickable.StopAtBounds
        delegate: Rectangle {
            id: card
            required property var modelData
            readonly property var entry: Shell.apps.get(Shell.apps.idForWindow(card.modelData.appId))
            width: list.width
            height: 64
            radius: Theme.radius
            color: cardTap.pressed ? Theme.surfacePressed : Theme.surfaceRaised
            border.color: card.modelData.activated ? Theme.accent : "transparent"
            border.width: 1

            AppIcon {
                id: icon
                anchors.left: parent.left
                anchors.leftMargin: Theme.space3
                anchors.verticalCenter: parent.verticalCenter
                size: 40
                source: card.entry.iconSource || ""
                name: card.entry.name || card.modelData.appId
            }
            Column {
                anchors.left: icon.right
                anchors.leftMargin: Theme.space3
                anchors.right: closeButton.left
                anchors.verticalCenter: parent.verticalCenter
                Text {
                    width: parent.width
                    text: card.entry.name || card.modelData.appId || "Unknown app"
                    color: Theme.text
                    font.pixelSize: Theme.fontBody
                    font.weight: Font.Medium
                    elide: Text.ElideRight
                }
                Text {
                    width: parent.width
                    text: card.modelData.title || ""
                    color: Theme.textMuted
                    font.pixelSize: Theme.fontLabel
                    elide: Text.ElideRight
                }
            }
            TapHandler {
                id: cardTap
                gesturePolicy: TapHandler.ReleaseWithinBounds
                onTapped: Shell.activateWindow(card.modelData)
            }
            IconButton {
                id: closeButton
                anchors.right: parent.right
                anchors.verticalCenter: parent.verticalCenter
                icon: "close"
                accessibleName: "Close " + (card.entry.name || card.modelData.appId)
                onActivated: Shell.closeWindow(card.modelData)
            }
        }
    }
    Text {
        anchors.centerIn: parent
        visible: Shell.windowCount === 0
        text: "No open windows"
        color: Theme.textMuted
        font.pixelSize: Theme.fontBody
    }
}
