import QtQuick
import QtQuick.Shapes
import "../theme"

// Busy indicator: a 3/4 arc rotated on the render thread, only while visible and running.
Item {
    id: root
    property int size: 18
    property color color: Theme.accent
    property bool running: visible
    implicitWidth: size
    implicitHeight: size

    Shape {
        id: arc
        anchors.fill: parent
        preferredRendererType: Shape.CurveRenderer
        ShapePath {
            strokeColor: root.color
            strokeWidth: Math.max(2, root.size / 9)
            fillColor: "transparent"
            capStyle: ShapePath.RoundCap
            PathAngleArc {
                centerX: root.size / 2; centerY: root.size / 2
                radiusX: root.size / 2 - 2; radiusY: root.size / 2 - 2
                startAngle: 0; sweepAngle: 270
            }
        }
        RotationAnimator on rotation {
            from: 0; to: 360; duration: 900
            loops: Animation.Infinite
            running: root.running && root.visible
        }
    }
}
