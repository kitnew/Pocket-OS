import QtQuick
import "../theme"

// Rounded surface for grouping content (panels, cards). Children go into `content`
// and are laid out by the caller; `padding` is applied around them.
Rectangle {
    id: root
    default property alias content: inner.data
    property int padding: Theme.space4

    color: Theme.surface
    radius: Theme.radiusLarge
    border.color: Theme.outline
    border.width: 1
    implicitWidth: inner.childrenRect.width + 2 * padding
    implicitHeight: inner.childrenRect.height + 2 * padding

    Item {
        id: inner
        anchors.fill: parent
        anchors.margins: root.padding
    }
}
