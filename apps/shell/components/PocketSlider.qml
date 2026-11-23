import QtQuick
import "../theme"

// Basic horizontal slider (0..1). `value` shows the real state; dragging emits
// `moved(v)` continuously and the caller applies it (and feeds the result back).
Item {
    id: root

    property real value: 0
    property string icon
    property bool supported: true
    signal moved(real v)

    readonly property real shown: drag.active || tap.pressed ? local : value
    property real local: value

    implicitHeight: Theme.touchTarget
    implicitWidth: 240
    opacity: supported ? 1 : 0.4
    enabled: supported

    Rectangle {
        id: track
        anchors.fill: parent
        radius: height / 2
        color: Theme.surfaceRaised
        clip: true
        Rectangle {
            height: parent.height
            width: Math.max(height, parent.width * root.shown)
            radius: height / 2
            color: Theme.accent
        }
    }
    Icon {
        anchors.verticalCenter: parent.verticalCenter
        anchors.left: parent.left
        anchors.leftMargin: (Theme.touchTarget - Theme.icon) / 2
        name: root.icon
        visible: root.icon.length > 0
    }

    function set(x) {
        local = Math.max(0, Math.min(1, x / width))
        moved(local)
    }
    DragHandler {
        id: drag
        target: null
        xAxis.enabled: true
        yAxis.enabled: false
        onCentroidChanged: if (active) root.set(centroid.position.x)
    }
    TapHandler {
        id: tap
        onPressedChanged: if (pressed) root.set(point.position.x)
    }
}
