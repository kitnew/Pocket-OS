import QtQuick
import "../../state"
import "../../theme"
import "../dock"
import "../launcher"
import "../switcher"

// The home surface content: one of three pages (UI-owned Shell.homePage). Switching
// pages is a short cross-fade; the inactive pages are not rendered. In Dock Mode (the
// core's mode) the "home" page is the Dock control surface; launcher and switcher stay.
Rectangle {
    id: root
    color: Theme.background
    readonly property bool landscape: width > height

    HomePage {
        anchors.fill: parent
        visible: opacity > 0
        opacity: Shell.homePage === "home" && !Shell.docked ? 1 : 0
        Behavior on opacity { NumberAnimation { duration: Theme.duration } }
    }
    DockView {
        anchors.fill: parent
        visible: opacity > 0
        opacity: Shell.homePage === "home" && Shell.docked ? 1 : 0
        Behavior on opacity { NumberAnimation { duration: Theme.duration } }
    }
    LauncherPage {
        id: launcher
        anchors.fill: parent
        visible: opacity > 0
        opacity: Shell.homePage === "apps" ? 1 : 0
        Behavior on opacity { NumberAnimation { duration: Theme.duration } }
        onVisibleChanged: if (!visible) reset()
    }
    SwitcherPage {
        anchors.fill: parent
        visible: opacity > 0
        opacity: Shell.homePage === "switcher" ? 1 : 0
        Behavior on opacity { NumberAnimation { duration: Theme.duration } }
    }

    // Layout diagnostics: the surface is configured asynchronously and sizes arrive one
    // dimension at a time; report only settled, real changes.
    property var reported: undefined
    function report() {
        if (width <= 0 || height <= 0 || landscape === reported)
            return
        const first = reported === undefined
        reported = landscape
        Shell.diag(first ? "shell.started" : "layout.orientation",
                   { runtime: "Quickshell", surface: "home", size: width + "x" + height, landscape: landscape })
    }
    onWidthChanged: Qt.callLater(report)
    onHeightChanged: Qt.callLater(report)
}
