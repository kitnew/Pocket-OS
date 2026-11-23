import QtQuick
import "../theme"

// A monochrome UI icon from theme/icons (white). Size via `size`; dim via opacity.
Image {
    property string name
    property int size: Theme.icon
    source: name ? Theme.iconUrl(name) : ""
    width: size
    height: size
    sourceSize: Qt.size(size * 2, size * 2) // crisp after the compositor's scale
    fillMode: Image.PreserveAspectFit
    smooth: true
    asynchronous: false
}
