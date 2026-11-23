import QtQuick
import "../../components"
import "../../state"
import "../../theme"

// Modal confirmation for actions the laptop marks `confirm` and for re-sending an action
// whose outcome is unknown. A guard against accidental taps, NOT a security boundary
// (any app on the phone could tap it through the compositor; issue #8, ADR-0008).
Rectangle {
    id: root
    color: Theme.scrim
    TapHandler { onTapped: Shell.cancelDockConfirm() }   // tap outside = cancel

    Rectangle {
        anchors.centerIn: parent
        width: Math.min(parent.width - 2 * Theme.space5, 440)
        height: col.implicitHeight + 2 * Theme.space4
        radius: Theme.radiusLarge
        color: Theme.surface
        border.color: Theme.outline
        TapHandler {}   // swallow taps on the card
        Column {
            id: col
            anchors.fill: parent
            anchors.margins: Theme.space4
            spacing: Theme.space4
            Text {
                width: parent.width
                wrapMode: Text.Wrap
                text: Shell.dockConfirm ? Shell.dockConfirm.text : ""
                color: Theme.text
                font.pixelSize: Theme.fontBody
            }
            Row {
                anchors.right: parent.right
                spacing: Theme.space2
                PocketButton { text: "Cancel"; onActivated: Shell.cancelDockConfirm() }
                PocketButton {
                    primary: true
                    text: Shell.dockConfirm && Shell.dockConfirm.retry > 0 ? "Send again" : "Confirm"
                    onActivated: Shell.acceptDockConfirm()
                }
            }
        }
    }
}
