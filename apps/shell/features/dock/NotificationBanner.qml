import QtQuick
import "../../state"
import "../../theme"

// A laptop notification (Context Overlay, non-modal). Tap dismisses it on the phone only.
Rectangle {
    id: root
    property var notification: null
    visible: notification !== null
    height: 60
    radius: Theme.radius
    color: Theme.surfaceRaised
    border.color: notification && notification.urgency === 2 ? Theme.danger : Theme.outline
    border.width: 1

    Column {
        anchors.fill: parent
        anchors.margins: Theme.space2
        anchors.leftMargin: Theme.space3
        spacing: 1
        Text {
            width: parent.width
            text: root.notification ? (root.notification.app || "Laptop") + " · " + (root.notification.summary || "") : ""
            color: Theme.text
            font.pixelSize: Theme.fontLabel
            font.weight: Font.DemiBold
            elide: Text.ElideRight
        }
        Text {
            width: parent.width
            text: !root.notification ? ""
                : root.notification.redacted ? "Hidden while the laptop is locked"
                : (root.notification.body || "")
            color: Theme.textMuted
            font.pixelSize: Theme.fontCaption
            maximumLineCount: 2
            wrapMode: Text.Wrap
            elide: Text.ElideRight
        }
    }
    TapHandler { onTapped: Shell.dismissDockBanner() }
}
