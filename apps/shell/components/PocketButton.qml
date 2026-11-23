import QtQuick
import "../theme"

// Text button (optionally with a leading icon). Variants: "tonal" (default), "primary"
// (accent), "danger", "ghost" (no background). It knows nothing about what it triggers:
// callers handle `activated(point)`.
Pressable {
    id: root

    property alias text: label.text
    property string icon
    property bool checked: false   // Stage 1 name; same look as primary
    property bool primary: checked
    property string variant: primary ? "primary" : "tonal"
    property bool compact: false
    signal activated(var point)

    implicitWidth: Math.max(compact ? Theme.touchTarget : Theme.touchTarget * 2.2,
                            content.implicitWidth + 2 * (compact ? Theme.space3 : Theme.space5))
    implicitHeight: compact ? 40 : Theme.touchTarget
    opacity: enabled ? 1.0 : 0.4   // disabled items receive no pointer events
    pressScale: 0.97
    Accessible.role: Accessible.Button
    Accessible.name: text
    onTapped: (point) => activated(point)


    Rectangle {
        anchors.fill: parent
        radius: height / 2
        color: root.pressed ? Theme.surfacePressed
             : root.variant === "primary" ? Theme.accent
             : root.variant === "danger" ? Theme.dangerSoft
             : root.variant === "ghost" ? "transparent" : Theme.surfaceHigh
        Behavior on color { ColorAnimation { duration: Theme.durationFast } }
    }
    Row {
        id: content
        anchors.centerIn: parent
        spacing: Theme.space2
        Icon {
            anchors.verticalCenter: parent.verticalCenter
            visible: root.icon.length > 0
            name: root.icon
            size: Theme.iconSmall + 2
        }
        Text {
            id: label
            anchors.verticalCenter: parent.verticalCenter
            color: root.variant === "primary" ? Theme.accentText
                 : root.variant === "danger" ? Theme.danger : Theme.text
            font.pixelSize: root.compact ? Theme.fontLabel : Theme.fontBody
            font.weight: Font.DemiBold
        }
    }
}
