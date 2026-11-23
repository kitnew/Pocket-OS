import QtQuick
import "../../components"
import "../../state"
import "../../theme"

// Modal confirmation for actions the laptop marks `confirm` and for re-sending an action
// whose outcome is unknown. A guard against accidental taps, NOT a security boundary
// (any app on the phone could tap it through the compositor; issue #8, ADR-0008).
// Cancel is the safe default; a tap outside cancels.
Sheet {
    id: root
    open: Shell.dockConfirm !== null
    cardWidth: Math.min(width - 2 * Theme.gutter, 440)
    onDismissed: Shell.cancelDockConfirm()

    Column {
        width: parent.width
        spacing: Theme.space4
        Row {
            spacing: Theme.space3
            Rectangle {
                width: 40; height: 40; radius: 12
                color: Theme.warningSoft
                Icon { anchors.centerIn: parent; name: Shell.dockConfirm && Shell.dockConfirm.retry > 0 ? "refresh" : (Shell.dockConfirm && Shell.dockConfirm.tile.icon) || "alert" }
            }
            Text {
                anchors.verticalCenter: parent.verticalCenter
                text: Shell.dockConfirm ? (Shell.dockConfirm.retry > 0 ? "Send again?" : Shell.dockConfirm.label + "?") : ""
                color: Theme.text
                font.pixelSize: Theme.fontTitle
                font.weight: Font.DemiBold
            }
        }
        Text {
            width: parent.width
            wrapMode: Text.Wrap
            text: Shell.dockConfirm ? Shell.dockConfirm.text : ""
            color: Theme.textMuted
            font.pixelSize: Theme.fontBody
        }
        Row {
            anchors.right: parent.right
            spacing: Theme.space2
            PocketButton { variant: "ghost"; text: "Cancel"; onActivated: Shell.cancelDockConfirm() }
            PocketButton {
                variant: "primary"
                text: Shell.dockConfirm && Shell.dockConfirm.retry > 0 ? "Send again" : (Shell.dockConfirm ? Shell.dockConfirm.label : "Confirm")
                onActivated: Shell.acceptDockConfirm()
            }
        }
    }
}
