import QtQuick
import "../theme"

// A labelled thin bar (0..100), e.g. CPU and memory. `stale` dims it.
Column {
    id: root
    property string label
    property real value: 0
    property bool stale: false
    property color color: Theme.accent
    spacing: 6
    opacity: stale ? 0.45 : 1

    Item {
        width: parent.width
        height: valueText.implicitHeight
        Text {
            text: root.label
            color: Theme.textMuted
            font.pixelSize: Theme.fontCaption
            font.weight: Font.Medium
            font.letterSpacing: 0.6
        }
        Text {
            id: valueText
            anchors.right: parent.right
            text: Math.round(root.value) + "%"
            color: Theme.text
            font.pixelSize: Theme.fontLabel
            font.weight: Font.DemiBold
            font.features: { "tnum": 1 }
        }
    }
    Rectangle {
        width: parent.width
        height: 4
        radius: 2
        color: Theme.surfaceHigh
        Rectangle {
            width: parent.width * Math.max(0, Math.min(1, root.value / 100))
            height: parent.height
            radius: 2
            color: root.value >= 90 ? Theme.danger : root.value >= 75 ? Theme.warning : root.color
            Behavior on width { NumberAnimation { duration: Theme.durationSlow; easing.type: Theme.easing } }
        }
    }
}
