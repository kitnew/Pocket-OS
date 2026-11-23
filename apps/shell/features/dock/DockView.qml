pragma ComponentBehavior: Bound
import QtQuick
import "../../components"
import "../../state"
import "../../theme"

// The Dock surface: immersive landscape (780×360 logical px on lavender; the status and
// navigation bars are unmapped while it shows, Shell.immersive). Three layers, stacked as
// siblings so an overlay never replaces the view under it
// (docs/dock-mode/ux-specification.md):
//   Base View          AmbientHome | ControlDeck | LaptopView   (Shell.dockNav.view)
//   Context Overlay    notification banner (non-modal); confirmation, tile details, tile
//                      editor and notification history sheets (modal: they suspend idle
//                      return and take all input)
//   Global Interaction reserved for voice sessions (future; suspends idle return)
// Navigation rules live in state/navmodel.js; this file only renders them.
//
// System access without the bars: the rail's Controls button and a swipe down from the
// top edge open Quick Settings (Leave Dock, Phone apps, toggles); Phone apps brings the
// bars back. Nothing critical is gesture-only.
Item {
    id: root
    readonly property var nav: Shell.dockNav
    readonly property var views: ["home", "deck", "link"]
    readonly property int current: Math.max(0, views.indexOf(nav.view))

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
        value: root.visible && Shell.display.on
    }

    Rectangle { anchors.fill: parent; color: Theme.background }

    // ── Rail ─────────────────────────────────────────────────────────────────────────
    Rectangle {
        id: rail
        width: Theme.railWidth
        height: parent.height
        color: Theme.surface

        Column {
            anchors.top: parent.top
            anchors.topMargin: Theme.space3
            anchors.horizontalCenter: parent.horizontalCenter
            spacing: Theme.space2
            Repeater {
                model: [{ view: "home", icon: "home", name: "Home" },
                        { view: "deck", icon: "grid", name: "Control Deck" },
                        { view: "link", icon: "laptop", name: "Laptop" }]
                delegate: Pressable {
                    id: item
                    required property var modelData
                    required property int index
                    readonly property bool selected: root.nav.view === modelData.view
                    width: 52; height: 48
                    pressScale: 0.92
                    Accessible.role: Accessible.Button
                    Accessible.name: modelData.name
                    onTapped: Shell.dockNav.navigate(modelData.view)
                    Rectangle {
                        anchors.fill: parent
                        radius: 16
                        color: item.pressed ? Theme.surfacePressed : item.selected ? Theme.accentSoft : "transparent"
                        Behavior on color { ColorAnimation { duration: Theme.durationFast } }
                    }
                    Icon {
                        anchors.centerIn: parent
                        name: item.modelData.icon
                        opacity: item.selected ? 1 : 0.7
                    }
                    // Laptop link state on the Laptop item (green/amber/red/grey).
                    Rectangle {
                        visible: item.modelData.view === "link"
                        x: parent.width / 2 + 6; y: 10
                        width: 9; height: 9; radius: 4.5
                        border.width: 2
                        border.color: Theme.surface
                        color: Shell.dockConnected ? Theme.success
                             : Shell.dock.state === "connecting" || Shell.dock.retryScheduled ? Theme.warning
                             : Shell.dock.state === "error" ? Theme.danger : Theme.textDisabled
                    }
                    // Selected marker on the outer edge.
                    Rectangle {
                        visible: item.selected
                        x: -(Theme.railWidth - 52) / 2
                        anchors.verticalCenter: parent.verticalCenter
                        width: 3; height: 20; radius: 1.5
                        color: Theme.accent
                    }
                }
            }
        }
        Column {
            anchors.bottom: parent.bottom
            anchors.bottomMargin: Theme.space3
            anchors.horizontalCenter: parent.horizontalCenter
            spacing: Theme.space2
            IconButton {
                anchors.horizontalCenter: parent.horizontalCenter
                width: 48; height: 48
                visible: root.nav.view !== "home"
                icon: "pin"
                accessibleName: root.nav.pinned ? "Unpin view" : "Pin view (no return to Home)"
                checked: root.nav.pinned
                onActivated: Shell.dockNav.setPinned(!root.nav.pinned)
            }
            IconButton {
                anchors.horizontalCenter: parent.horizontalCenter
                width: 48; height: 48
                icon: "sliders"
                accessibleName: "Controls"
                checked: Shell.panel === "quicksettings"
                onActivated: Shell.togglePanel("quicksettings")
            }
        }
    }

    // ── Base View ────────────────────────────────────────────────────────────────────
    Item {
        id: content
        anchors.left: rail.right
        anchors.right: parent.right
        height: parent.height
        clip: true

        Repeater {
            model: 3
            delegate: Loader {
                id: view
                required property int index
                readonly property bool isCurrent: root.current === index
                width: content.width
                height: content.height
                // Slide from the direction of travel; the outgoing view fades out faster
                // than the incoming one fades in, so the two never read as one screen.
                x: isCurrent ? 0 : (index < root.current ? -Theme.travel : Theme.travel)
                opacity: isCurrent ? 1 : 0
                visible: opacity > 0
                Behavior on x { NumberAnimation { duration: Theme.durationSlow; easing.type: Theme.easingEnter } }
                Behavior on opacity {
                    NumberAnimation { duration: view.isCurrent ? Theme.duration : Theme.durationFast }
                }
                sourceComponent: index === 0 ? homeView : index === 1 ? deckView : laptopView
            }
        }
        Component { id: homeView; AmbientHome {} }
        Component { id: deckView; ControlDeck {} }
        Component { id: laptopView; LaptopView {} }
    }

    // ── Top edge: swipe down for Controls (Quick Settings) ─────────────────────────
    Item {
        anchors.left: content.left
        anchors.right: content.right
        height: 18
        Rectangle {
            anchors.horizontalCenter: parent.horizontalCenter
            y: 5
            width: 36; height: 4; radius: 2
            color: Theme.textDisabled
            opacity: edge.active ? 0.9 : 0.35
        }
        DragHandler {
            id: edge
            target: null
            xAxis.enabled: false
            onActiveChanged: if (!active && centroid.position.y - centroid.pressPosition.y > 36)
                                 Shell.togglePanel("quicksettings")
        }
    }

    // ── Context Overlay: non-modal banner ───────────────────────────────────────────
    NotificationBanner {
        anchors.horizontalCenter: content.horizontalCenter
        width: Math.min(content.width - 2 * Theme.space4, 520)
        notification: Shell.dockBanner
    }

    // ── Context Overlay: modal sheets (cover the rail too) ──────────────────────────
    TileDetailSheet { anchors.fill: parent }
    TileEditorSheet { anchors.fill: parent }
    NotificationsSheet { anchors.fill: parent }
    ConfirmSheet { anchors.fill: parent }

    // ── Global Interaction (reserved; Stage 5+ voice, issue #18) ────────────────────
    Item {
        anchors.fill: parent
        visible: root.nav.globalInteraction !== ""
    }
}
