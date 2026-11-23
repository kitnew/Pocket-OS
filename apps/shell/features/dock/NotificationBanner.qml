import QtQuick
import "../../components"
import "../../state"
import "../../theme"

// A laptop notification (Context Overlay, non-modal): slides in from the top edge,
// hides by itself after 6 s (critical ones stay). Tap opens the history; a swipe up or
// sideways dismisses it (on the phone only).
Item {
    id: root
    property var notification: null
    property var shownNotification: null   // kept during the exit animation
    readonly property bool active: notification !== null
    height: 64
    y: active ? Theme.space3 : -height - Theme.space3
    opacity: active ? 1 : 0
    visible: opacity > 0
    z: 50
    onNotificationChanged: if (notification !== null) shownNotification = notification
    Behavior on y { NumberAnimation { duration: Theme.durationSlow; easing.type: root.active ? Theme.easingEnter : Theme.easingExit } }
    Behavior on opacity { NumberAnimation { duration: Theme.duration } }

    Rectangle {
        id: card
        width: parent.width
        height: parent.height
        x: drag.active ? drag.translation.x : 0
        y: drag.active ? Math.min(0, drag.translation.y) : 0
        radius: Theme.radius
        color: Theme.surfaceHigh
        border.width: 1
        border.color: root.shownNotification && root.shownNotification.urgency === 2 ? Theme.danger : Theme.outlineStrong
        Behavior on x { enabled: !drag.active; NumberAnimation { duration: Theme.duration } }

        Rectangle {
            id: well
            x: Theme.space3
            anchors.verticalCenter: parent.verticalCenter
            width: 36; height: 36; radius: 11
            color: Theme.accentSoft
            Icon { anchors.centerIn: parent; name: "bell"; size: 18 }
        }
        Column {
            anchors.left: well.right
            anchors.leftMargin: Theme.space3
            anchors.right: parent.right
            anchors.rightMargin: Theme.space3
            anchors.verticalCenter: parent.verticalCenter
            spacing: 2
            Text {
                width: parent.width
                text: root.shownNotification ? (root.shownNotification.app || "Laptop") + " · " + (root.shownNotification.summary || "") : ""
                color: Theme.text
                font.pixelSize: Theme.fontLabel
                font.weight: Font.DemiBold
                elide: Text.ElideRight
            }
            Text {
                width: parent.width
                text: !root.shownNotification ? ""
                    : root.shownNotification.redacted ? "Hidden while the laptop is locked"
                    : (root.shownNotification.body || "")
                visible: text.length > 0
                color: Theme.textMuted
                font.pixelSize: Theme.fontCaption
                maximumLineCount: 1
                elide: Text.ElideRight
            }
        }
        TapHandler {
            onTapped: {
                Shell.dismissDockBanner()
                Shell.dockNav.openModal("notifications")
            }
        }
        DragHandler {
            id: drag
            target: null
            onActiveChanged: if (!active && (Math.abs(translation.x) > 60 || translation.y < -24))
                                 Shell.dismissDockBanner()
        }
    }
}
