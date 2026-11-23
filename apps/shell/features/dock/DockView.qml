pragma ComponentBehavior: Bound
import QtQuick
import "../../components"
import "../../state"
import "../../theme"

// The Dock surface (landscape, 780×288 logical px on lavender). Three layers, stacked as
// siblings so an overlay never replaces the view under it
// (docs/dock-mode/ux-specification.md):
//   Base View          AmbientHome | ControlDeck | LinkView   (Shell.dockNav.view)
//   Context Overlay    notification banner (non-modal); confirmation and notification
//                      history sheets (modal: they suspend idle return)
//   Global Interaction reserved for voice sessions (future; suspends idle return)
// Navigation rules live in state/navmodel.js; this file only renders them.
Item {
    id: root
    readonly property var nav: Shell.dockNav

    // Every touch on the Dock surface counts as interaction (resets the idle timer). A
    // PointHandler only grabs passively, so it never takes the touch from a control.
    PointHandler {
        onActiveChanged: if (active) Shell.dockNav.interact()
    }

    // The reconnect countdown ticks (once per second, in DockClient) only while a view
    // that shows it is on a lit screen; the deadline itself is Core's.
    Binding {
        target: Shell.dock
        property: "countdownActive"
        value: root.visible && Shell.display.on && root.nav.view !== "deck"
    }

    // ── Rail ─────────────────────────────────────────────────────────────────────────
    Rectangle {
        id: rail
        width: 56
        height: parent.height
        color: Theme.surface

        Column {
            anchors.top: parent.top
            anchors.topMargin: Theme.space2
            anchors.horizontalCenter: parent.horizontalCenter
            spacing: Theme.space1
            Repeater {
                model: [{ view: "home", icon: "home", name: "Home" },
                        { view: "deck", icon: "grid", name: "Control Deck" },
                        { view: "link", icon: "plug", name: "Link" }]
                delegate: IconButton {
                    required property var modelData
                    icon: modelData.icon
                    accessibleName: modelData.name
                    checked: root.nav.view === modelData.view
                    onActivated: Shell.dockNav.navigate(modelData.view)
                }
            }
        }
        IconButton {
            anchors.bottom: parent.bottom
            anchors.bottomMargin: Theme.space2
            anchors.horizontalCenter: parent.horizontalCenter
            icon: "pin"
            accessibleName: root.nav.pinned ? "Unpin view" : "Pin view"
            checked: root.nav.pinned
            onActivated: Shell.dockNav.setPinned(!root.nav.pinned)
        }
    }

    // ── Base View ────────────────────────────────────────────────────────────────────
    Item {
        id: content
        anchors.left: rail.right
        anchors.right: parent.right
        height: parent.height

        AmbientHome {
            anchors.fill: parent
            visible: opacity > 0
            opacity: root.nav.view === "home" ? 1 : 0
            Behavior on opacity { NumberAnimation { duration: Theme.durationFast } }
        }
        ControlDeck {
            anchors.fill: parent
            visible: opacity > 0
            opacity: root.nav.view === "deck" ? 1 : 0
            Behavior on opacity { NumberAnimation { duration: Theme.durationFast } }
        }
        LinkView {
            anchors.fill: parent
            visible: opacity > 0
            opacity: root.nav.view === "link" ? 1 : 0
            Behavior on opacity { NumberAnimation { duration: Theme.durationFast } }
        }

        // ── Context Overlay: non-modal banner ───────────────────────────────────────
        NotificationBanner {
            anchors.top: parent.top
            anchors.topMargin: Theme.space2
            anchors.horizontalCenter: parent.horizontalCenter
            width: Math.min(parent.width - 2 * Theme.space4, 520)
            notification: Shell.dockBanner
        }
    }

    // ── Context Overlay: modal sheets (cover the rail too) ──────────────────────────
    ConfirmSheet {
        anchors.fill: parent
        visible: Shell.dockConfirm !== null
    }
    NotificationsSheet {
        anchors.fill: parent
        visible: root.nav.modal === "notifications"
    }

    // ── Global Interaction (reserved; Stage 5+ voice, issue #18) ────────────────────
    Item {
        anchors.fill: parent
        visible: root.nav.globalInteraction !== ""
    }
}
