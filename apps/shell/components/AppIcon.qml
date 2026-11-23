import QtQuick
import "../theme"

// Application icon from a resolved file URL; falls back to a lettered tile when the
// app has no icon or the file fails to load (missing icons are normal).
Item {
    id: root

    property string source
    property string name
    property int size: Theme.appIcon

    implicitWidth: size
    implicitHeight: size

    readonly property bool hasImage: img.status === Image.Ready

    Image {
        id: img
        anchors.fill: parent
        source: root.source
        sourceSize: Qt.size(root.size * 3, root.size * 3)
        fillMode: Image.PreserveAspectFit
        asynchronous: true
        smooth: true
        visible: root.hasImage
    }
    Rectangle {
        anchors.fill: parent
        visible: !root.hasImage
        radius: Theme.radius
        // Stable color per name.
        color: Qt.hsla((Math.abs(root.hash(root.name)) % 360) / 360, 0.45, 0.42, 1)
        Text {
            anchors.centerIn: parent
            text: root.name ? root.name.charAt(0).toUpperCase() : "?"
            color: "white"
            font.pixelSize: root.size * 0.46
            font.weight: Font.DemiBold
        }
    }

    function hash(s) {
        let h = 0
        for (let i = 0; i < (s || "").length; ++i)
            h = (h * 31 + s.charCodeAt(i)) | 0
        return h
    }
}
