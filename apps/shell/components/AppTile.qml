import QtQuick
import "../theme"

// Launcher/home tile: app icon + name. Optional `running` dot. Callers handle `activated`.
Item {
    id: root

    property string name
    property string iconSource
    property bool running: false
    signal activated()

    implicitWidth: 76
    implicitHeight: Theme.appIcon + Theme.space2 + label.implicitHeight + Theme.space2
    Accessible.role: Accessible.Button
    Accessible.name: name

    Rectangle {
        anchors.fill: parent
        radius: Theme.radius
        color: Theme.surfacePressed
        opacity: tap.pressed ? 1 : 0
        Behavior on opacity { NumberAnimation { duration: Theme.durationFast } }
    }
    AppIcon {
        id: icon
        anchors.horizontalCenter: parent.horizontalCenter
        anchors.top: parent.top
        anchors.topMargin: Theme.space1
        source: root.iconSource
        name: root.name
        scale: tap.pressed ? 0.92 : 1
        Behavior on scale { NumberAnimation { duration: Theme.durationFast } }
    }
    Rectangle {
        visible: root.running
        anchors.horizontalCenter: parent.horizontalCenter
        anchors.top: icon.bottom
        anchors.topMargin: 2
        width: 5; height: 5; radius: 2.5
        color: Theme.accent
    }
    Text {
        id: label
        anchors.top: icon.bottom
        anchors.topMargin: Theme.space2
        anchors.horizontalCenter: parent.horizontalCenter
        width: parent.width - 4
        horizontalAlignment: Text.AlignHCenter
        text: root.name
        color: Theme.text
        font.pixelSize: Theme.fontLabel
        elide: Text.ElideRight
        maximumLineCount: 1
    }
    TapHandler {
        id: tap
        gesturePolicy: TapHandler.ReleaseWithinBounds
        onTapped: root.activated()
    }
}
