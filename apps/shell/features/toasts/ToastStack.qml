pragma ComponentBehavior: Bound
import QtQuick
import "../../state"
import "../../theme"

// Transient messages (launch progress, saved/failed, access requests). Tap to dismiss.
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
            height: Math.max(44, label.implicitHeight + 2 * Theme.space3)
            radius: Theme.radius
            color: Theme.surfaceHigh
            border.color: toast.kind === "error" ? Theme.danger : Theme.outlineStrong
            border.width: 1
            opacity: 0
            Component.onCompleted: opacity = 1
            Behavior on opacity { NumberAnimation { duration: Theme.duration } }
            Rectangle {
                id: mark
                x: Theme.space3
                anchors.verticalCenter: parent.verticalCenter
                width: 8; height: 8; radius: 4
                color: toast.kind === "error" ? Theme.danger : Theme.accent
            }
            Text {
                id: label
                anchors.left: mark.right
                anchors.leftMargin: Theme.space3
                anchors.right: parent.right
                anchors.rightMargin: Theme.space3
                anchors.verticalCenter: parent.verticalCenter
                wrapMode: Text.Wrap
                maximumLineCount: 3
                elide: Text.ElideRight
                text: toast.text
                color: Theme.text
                font.pixelSize: Theme.fontLabel
            }
            TapHandler { onTapped: Shell.dismissToast(toast.serial) }
        }
    }
}
