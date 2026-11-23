pragma ComponentBehavior: Bound
import QtQuick
import "../../state"
import "../../theme"

// Transient messages (launch progress and failures). Tap to dismiss.
Column {
    id: root
    spacing: Theme.space2

    Repeater {
        model: Shell.toasts
        delegate: Rectangle {
            id: toast
            required property int serial
            required property string text
            required property string kind
            width: root.width
            height: label.implicitHeight + 2 * Theme.space3
            radius: Theme.radius
            color: toast.kind === "error" ? "#5a1d1d" : Theme.surfaceRaised
            border.color: toast.kind === "error" ? Theme.danger : Theme.outline
            border.width: 1
            Text {
                id: label
                anchors.fill: parent
                anchors.margins: Theme.space3
                verticalAlignment: Text.AlignVCenter
                wrapMode: Text.Wrap
                text: toast.text
                color: Theme.text
                font.pixelSize: Theme.fontLabel
            }
            TapHandler { onTapped: Shell.dismissToast(toast.serial) }
        }
    }
}
