// Stage 0 Qt Quick validation scene (plain Qt Quick, no shell runtime).
// Run inside the Phoc session:  qmlscene tests/device/qtquick-touch-test.qml
// Every touch press and button activation is logged to stderr with a "[touch-test]" prefix.
import QtQuick
import QtQuick.Window

Window {
    id: win
    visible: true
    visibility: Window.FullScreen
    color: "#101418"
    title: "Pocket OS Qt Quick test"

    property int taps: 0
    property int frames: 0

    Component.onCompleted: console.log("[touch-test] started", width + "x" + height,
                                       "dpr=" + Screen.devicePixelRatio,
                                       "orientation=" + Screen.orientation)
    onWidthChanged: console.log("[touch-test] resized", width + "x" + height)

    // Frame counter: proves continuous rendering and lets us estimate fps.
    FrameAnimation {
        running: true
        onTriggered: win.frames++
    }
    Timer {
        interval: 5000; running: true; repeat: true
        property int last: 0
        onTriggered: { console.log("[touch-test] fps≈" + ((win.frames - last) / 5).toFixed(1)); last = win.frames }
    }

    // Logs every raw touch point, anywhere on screen.
    MultiPointTouchArea {
        anchors.fill: parent
        mouseEnabled: false
        onPressed: (points) => points.forEach(p =>
            console.log("[touch-test] touch press id=" + p.pointId + " x=" + p.x.toFixed(0) + " y=" + p.y.toFixed(0)))
    }

    Column {
        anchors.centerIn: parent
        spacing: 24

        Text {
            anchors.horizontalCenter: parent.horizontalCenter
            text: "Qt Quick on Phoc"
            color: "white"; font.pixelSize: 28
        }
        Rectangle {
            id: button
            width: 240; height: 96; radius: 16
            anchors.horizontalCenter: parent.horizontalCenter
            color: tap.pressed ? "#2f81f7" : (win.taps % 2 ? "#238636" : "#30363d")
            Text { anchors.centerIn: parent; text: "Tap me (" + win.taps + ")"; color: "white"; font.pixelSize: 24 }
            TapHandler {
                id: tap
                onTapped: (point) => { win.taps++; console.log("[touch-test] button tapped count=" + win.taps + " device=" + point.device.name) }
            }
        }
        Text {
            anchors.horizontalCenter: parent.horizontalCenter
            text: win.width + "×" + win.height + " @" + Screen.devicePixelRatio
            color: "#8b949e"; font.pixelSize: 16
        }
    }
}
