import QtQuick
import "../../components"
import "../../state"
import "../../theme"

// System navigation: keyboard, Home, open windows. Always available (also over apps).
Rectangle {
    color: Theme.background

    Row {
        anchors.centerIn: parent
        spacing: Math.max(Theme.space5, (parent.width - 3 * Theme.touchTarget) / 5)

        IconButton {
            icon: "keyboard"
            accessibleName: "Keyboard"
            enabled: Shell.osk.available
            checked: Shell.osk.visible
            onActivated: {
                Shell.diag("nav.keyboard", { visible: !Shell.osk.visible })
                Shell.osk.toggle()
            }
        }
        IconButton {
            icon: "home"
            accessibleName: "Home"
            checked: Shell.homeVisible && Shell.homePage === "home" && Shell.panel === ""
            onActivated: Shell.goHome()
        }
        IconButton {
            id: windowsButton
            icon: "windows"
            accessibleName: "Open windows"
            checked: Shell.homeVisible && Shell.homePage === "switcher" && Shell.panel === ""
            onActivated: Shell.showPage("switcher")
            Rectangle {
                visible: Shell.windowCount > 0
                anchors.right: parent.right
                anchors.top: parent.top
                anchors.margins: 4
                width: Math.max(16, count.implicitWidth + 6); height: 16; radius: 8
                color: Theme.accent
                Text {
                    id: count
                    anchors.centerIn: parent
                    text: Shell.windowCount
                    color: Theme.accentText
                    font.pixelSize: Theme.fontCaption
                    font.weight: Font.DemiBold
                }
            }
        }
    }
}
