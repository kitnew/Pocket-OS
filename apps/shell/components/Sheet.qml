import QtQuick
import "../theme"

// Modal sheet: a scrim over the whole parent and a card that rises into place. While
// open it takes every touch (nothing below reacts: dialogs win over background
// gestures). Tapping the scrim calls `dismissed()` unless `persistent`.
//   edge: "center" (dialog), "right" (side panel), "bottom" (bottom sheet)
Item {
    id: root
    property bool open: false
    property bool persistent: false
    property string edge: "center"
    property real cardWidth: Math.min(parent ? parent.width - 2 * Theme.gutter : 400, 460)
    property real cardHeight: -1          // -1 = content height (bounded by the parent)
    property real maxCardHeight: height - 2 * Theme.space3
    default property alias content: inner.data
    readonly property alias card: card
    property int padding: Theme.space4
    signal dismissed()

    visible: open || fade.running || scrim.opacity > 0
    z: 100

    Rectangle {
        id: scrim
        anchors.fill: parent
        color: Theme.scrim
        opacity: root.open ? 1 : 0
        Behavior on opacity { NumberAnimation { id: fade; duration: Theme.duration; easing.type: Theme.easing } }
        // Takes all input under the card; a tap outside dismisses.
        MouseArea {
            anchors.fill: parent
            enabled: root.open
            onClicked: if (!root.persistent) root.dismissed()
        }
    }

    Rectangle {
        id: card
        readonly property real contentHeight: inner.childrenRect.height + 2 * root.padding
        width: root.edge === "bottom" ? root.width : root.cardWidth
        height: root.edge === "right" ? root.height
              : Math.min(root.maxCardHeight, root.cardHeight >= 0 ? root.cardHeight : contentHeight)
        x: root.edge === "right" ? root.width - width + (root.open ? 0 : Theme.travel * 2)
         : (root.width - width) / 2
        y: root.edge === "right" ? 0
         : root.edge === "bottom" ? root.height - height + (root.open ? 0 : Theme.travel * 2)
         : (root.height - height) / 2 + (root.open ? 0 : Theme.travel / 2)
        opacity: root.open ? 1 : 0
        scale: root.edge === "center" ? (root.open ? 1 : 0.96) : 1
        radius: root.edge === "center" ? Theme.radiusLarge : 0
        color: Theme.surface
        border.width: 1
        border.color: Theme.outline
        Behavior on x { NumberAnimation { duration: Theme.duration; easing.type: root.open ? Theme.easingEnter : Theme.easingExit } }
        Behavior on y { NumberAnimation { duration: Theme.duration; easing.type: root.open ? Theme.easingEnter : Theme.easingExit } }
        Behavior on opacity { NumberAnimation { duration: Theme.duration } }
        Behavior on scale { NumberAnimation { duration: Theme.duration; easing.type: Theme.easingEnter } }
        // The card swallows taps (only the scrim dismisses).
        MouseArea { anchors.fill: parent; enabled: root.open }

        Item {
            id: inner
            anchors.fill: parent
            anchors.margins: root.padding
        }
    }
}
