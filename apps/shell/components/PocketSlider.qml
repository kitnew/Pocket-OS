import QtQuick
import "../theme"

// Horizontal slider (0..1) drawn as a filled pill with a leading icon. `value` shows the
// real state; dragging emits `moved(v)` continuously and the caller applies it (and
// feeds the result back). Only horizontal drags are taken, so it never fights a
// vertical scroll; a horizontal drag starting on it never reaches a page swipe.
Item {
    id: root

    property real value: 0
    property string icon
    property string label         // accessibility and the optional value text
    property bool showValue: true
    property bool supported: true
    signal moved(real v)
    signal iconTapped()

    readonly property bool active: drag.active || tap.pressed
    readonly property real shown: active ? local : value
    property real local: value

    implicitHeight: 48
    implicitWidth: 240
    opacity: supported ? 1 : 0.4
    enabled: supported
    Accessible.role: Accessible.Slider
    Accessible.name: label

    Rectangle {
        id: track
        anchors.fill: parent
        radius: height / 2
        color: Theme.surfaceHigh
        clip: true
        Rectangle {
            height: parent.height
            width: Math.max(height, parent.width * root.shown)
            radius: height / 2
            color: Theme.accent
            Behavior on width {
                enabled: !root.active
                NumberAnimation { duration: Theme.duration; easing.type: Theme.easing }
            }
        }
    }
    Icon {
        id: iconItem
        anchors.verticalCenter: parent.verticalCenter
        anchors.left: parent.left
        anchors.leftMargin: (root.height - Theme.icon) / 2
        name: root.icon
        visible: root.icon.length > 0
    }
    Text {
        anchors.verticalCenter: parent.verticalCenter
        anchors.right: parent.right
        anchors.rightMargin: Theme.space4
        visible: root.showValue && root.supported
        text: Math.round(root.shown * 100) + "%"
        color: root.shown > 0.88 ? Theme.accentText : Theme.textMuted
        font.pixelSize: Theme.fontLabel
        font.weight: Font.DemiBold
        font.features: { "tnum": 1 }
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
        grabPermissions: PointerHandler.CanTakeOverFromAnything
        onCentroidChanged: if (active) root.set(centroid.position.x)
    }
    TapHandler {
        id: tap
        onPressedChanged: if (pressed) root.set(point.position.x)
    }
}
