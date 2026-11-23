import QtQuick
import "../../components"
import "../../state"
import "../../theme"

// Quick Settings: real system controls and states (a tile is interactive only when the
// control works on this system; otherwise it is dimmed with the reason).
//
// Layout is bounded by the space that is really free: below the status bar and above the
// navigation bar when they are mapped, the full screen on the immersive Dock. Content that
// does not fit scrolls (vertical flick only, so sliders keep their horizontal drags).
// Landscape uses two columns (sliders + Dock actions | toggles); portrait one.
// Close: the ✕ button or a tap on the scrim.
Item {
    id: root
    readonly property var sys: Shell.system
    readonly property bool landscape: width > height
    readonly property real topInset: Shell.immersive ? Theme.space3 : Theme.statusBarHeight + Theme.space2
    readonly property real bottomInset: Shell.immersive ? Theme.space3 : Theme.navBarHeight + Theme.space2

    // Entrance: slide down + fade (the panel is unmapped on close, so exit is instant).
    // (The window maps/unmaps; this item's own `visible` never changes, so follow the panel.)
    readonly property bool wanted: Shell.panel === "quicksettings"
    property bool shown: false
    onWantedChanged: {
        shown = false
        if (wanted)
            Qt.callLater(() => root.shown = root.wanted)
    }
    Component.onCompleted: shown = wanted

    Rectangle {
        anchors.fill: parent
        color: Theme.scrim
        opacity: root.shown ? 1 : 0
        Behavior on opacity { NumberAnimation { duration: Theme.duration } }
        MouseArea {
            anchors.fill: parent
            onClicked: Shell.closePanel()
        }
    }

    Rectangle {
        id: card
        readonly property real padding: Theme.space4
        readonly property real available: root.height - root.topInset - root.bottomInset
        anchors.horizontalCenter: parent.horizontalCenter
        y: root.topInset - (root.shown ? 0 : Theme.travel)
        width: root.landscape ? Math.min(root.width - 2 * Theme.space3, 760) : root.width - 2 * Theme.space3
        height: Math.min(available, flick.contentHeight + 2 * padding)
        radius: Theme.radiusLarge
        color: Theme.surface
        border.color: Theme.outline
        border.width: 1
        opacity: root.shown ? 1 : 0
        Behavior on y { NumberAnimation { duration: Theme.duration; easing.type: Theme.easingEnter } }
        Behavior on opacity { NumberAnimation { duration: Theme.duration } }
        // Swallow taps on the card (only the scrim closes).
        MouseArea { anchors.fill: parent }

        Flickable {
            id: flick
            anchors.fill: parent
            anchors.margins: card.padding
            contentWidth: width
            contentHeight: body.height
            flickableDirection: Flickable.VerticalFlick
            boundsBehavior: Flickable.StopAtBounds
            interactive: contentHeight > height
            clip: true

            Column {
                id: body
                width: flick.width
                spacing: Theme.space3

                // ── Header ──────────────────────────────────────────────────────
                Item {
                    width: parent.width
                    height: 40
                    Column {
                        anchors.verticalCenter: parent.verticalCenter
                        spacing: 2
                        Text {
                            text: Shell.docked ? "Controls" : "Quick settings"
                            color: Theme.text
                            font.pixelSize: Theme.fontTitle
                            font.weight: Font.DemiBold
                        }
                        Text {
                            text: {
                                const parts = [Qt.formatDate(Shell.now, "ddd d MMM")]
                                // The time is shown here only when nothing else shows it.
                                if (!Shell.docked)
                                    parts.push(Qt.formatTime(Shell.now, "HH:mm"))
                                if (root.sys && root.sys.batteryAvailable)
                                    parts.push("Phone " + Math.round(root.sys.batteryPercent) + "%"
                                               + (root.sys.batteryFull ? " · full" : root.sys.batteryCharging ? " · charging" : ""))
                                return parts.join("  ·  ")
                            }
                            color: Theme.textMuted
                            font.pixelSize: Theme.fontLabel
                        }
                    }
                    IconButton {
                        anchors.right: parent.right
                        anchors.verticalCenter: parent.verticalCenter
                        icon: "close"
                        flat: false
                        accessibleName: "Close"
                        onActivated: Shell.closePanel()
                    }
                }

                Flow {
                    width: parent.width
                    spacing: Theme.space4

                    // ── Column A: levels and Dock actions ───────────────────────
                    Column {
                        id: colA
                        width: root.landscape ? (parent.width - Theme.space4) * 0.48 : parent.width
                        spacing: Theme.space3

                        PocketSlider {
                            width: parent.width
                            icon: "sun"
                            label: "Brightness"
                            supported: Shell.brightness.available
                            value: Shell.brightness.value
                            onMoved: (v) => Shell.brightness.setValue(v)
                        }
                        Text {
                            visible: Shell.brightness.lastError.length > 0
                            width: parent.width
                            wrapMode: Text.Wrap
                            text: "Brightness: " + Shell.brightness.lastError
                            color: Theme.danger
                            font.pixelSize: Theme.fontCaption
                        }
                        // Volume of the default output (PipeWire). External changes show up here.
                        Row {
                            id: volumeRow
                            width: parent.width
                            spacing: Theme.space2
                            readonly property var audio: Shell.audio
                            readonly property bool usable: audio !== null && audio.available
                            IconButton {
                                anchors.verticalCenter: parent.verticalCenter
                                width: 48; height: 48
                                icon: volumeRow.usable && !volumeRow.audio.muted && volumeRow.audio.volume > 0 ? "volume" : "volume-off"
                                accessibleName: volumeRow.usable && volumeRow.audio.muted ? "Unmute" : "Mute"
                                enabled: volumeRow.usable
                                checked: volumeRow.usable && volumeRow.audio.muted
                                checkedColor: Theme.danger
                                flat: false
                                onActivated: Shell.setMuted(!volumeRow.audio.muted)
                            }
                            PocketSlider {
                                width: parent.width - 48 - parent.spacing
                                anchors.verticalCenter: parent.verticalCenter
                                label: "Volume"
                                supported: volumeRow.usable
                                value: volumeRow.usable ? volumeRow.audio.volume : 0
                                onMoved: (v) => Shell.setVolume(v)
                            }
                        }
                        Text {
                            width: parent.width
                            elide: Text.ElideRight
                            text: Shell.volumeStatus
                            color: Shell.audio && Shell.audio.available ? Theme.textMuted : Theme.danger
                            font.pixelSize: Theme.fontCaption
                        }

                        // Dock: a clear way out, and to the phone's own apps. In landscape the
                        // momentary actions live here too, so every toggle fits without scrolling.
                        Flow {
                            width: parent.width
                            visible: Shell.docked || root.landscape
                            spacing: Theme.space2
                            PocketButton {
                                visible: Shell.docked
                                icon: "exit"
                                text: "Leave Dock"
                                compact: true
                                enabled: Shell.coreConnected && !Shell.core.requestPending
                                onActivated: { Shell.closePanel(); Shell.requestMode("handheld") }
                            }
                            PocketButton {
                                visible: Shell.docked
                                icon: "apps"
                                text: "Phone apps"
                                compact: true
                                onActivated: Shell.showPage("apps")
                            }
                            PocketButton {
                                visible: root.landscape
                                icon: "power"
                                text: "Screen off"
                                compact: true
                                enabled: Shell.display.available
                                onActivated: Shell.setDisplayOn(false)
                            }
                            PocketButton {
                                visible: root.landscape
                                icon: "code"
                                text: "Developer"
                                compact: true
                                variant: "ghost"
                                onActivated: Shell.panel = "dev"
                            }
                        }
                    }

                    // ── Column B: toggles ───────────────────────────────────────
                    Grid {
                        id: tiles
                        width: root.landscape ? (parent.width - Theme.space4) * 0.52 : parent.width
                        columns: 2
                        spacing: Theme.space2
                        readonly property real tileWidth: (width - (columns - 1) * spacing) / columns

                        ToggleTile {
                            width: tiles.tileWidth
                            dense: root.landscape
                            icon: root.sys && root.sys.wifiEnabled ? "wifi" : "wifi-off"
                            label: "Wi-Fi"
                            supported: root.sys !== null && root.sys.wifiAvailable && root.sys.wifiHardwareEnabled
                            checked: root.sys !== null && root.sys.wifiEnabled
                            detail: !root.sys || !root.sys.wifiAvailable ? "No Wi-Fi device"
                                  : !root.sys.wifiHardwareEnabled ? "Blocked by hardware switch"
                                  : !root.sys.wifiEnabled ? "Off"
                                  : root.sys.wifiConnected ? (root.sys.wifiName || "Connected") : "Not connected"
                            onActivated: {
                                Shell.diag("qs.wifi", { enable: !root.sys.wifiEnabled })
                                root.sys.setWifiEnabled(!root.sys.wifiEnabled)
                            }
                        }
                        ToggleTile {
                            width: tiles.tileWidth
                            dense: root.landscape
                            icon: "bluetooth"
                            label: "Bluetooth"
                            supported: root.sys !== null && root.sys.bluetoothAvailable
                            checked: root.sys !== null && root.sys.bluetoothEnabled
                            busy: root.sys !== null && root.sys.bluetoothBusy
                            detail: !root.sys || !root.sys.bluetoothAvailable ? "No adapter (bluetoothd)"
                                  : !root.sys.bluetoothEnabled ? "Off"
                                  : root.sys.bluetoothConnected > 0 ? root.sys.bluetoothConnected + " connected" : "On"
                            onActivated: {
                                Shell.diag("qs.bluetooth", { enable: !root.sys.bluetoothEnabled })
                                root.sys.setBluetoothEnabled(!root.sys.bluetoothEnabled)
                            }
                        }
                        // Dock Mode: the core's mode. The tile flips only when the core reports it.
                        ToggleTile {
                            width: tiles.tileWidth
                            dense: root.landscape
                            icon: "dock"
                            label: "Dock mode"
                            supported: Shell.coreConnected
                            busy: Shell.core.requestPending
                            checked: Shell.docked
                            detail: !Shell.coreConnected ? "Core not connected"
                                  : !Shell.dock.supported ? (Shell.docked ? "On" : "Off")
                                  : Shell.dock.state === "connected" ? (Shell.docked ? "On · " : "Off · ") + Shell.dock.hostName
                                  : Shell.docked ? "On · no laptop" : "Off"
                            onActivated: Shell.requestMode(Shell.docked ? "handheld" : "dock")
                        }
                        // Laptop notifications: Do Not Disturb (a Core preference).
                        ToggleTile {
                            width: tiles.tileWidth
                            dense: root.landscape
                            visible: Shell.dock.platform
                            icon: "moon"
                            label: "Do not disturb"
                            checked: Shell.dock.preferences.dnd === true
                            detail: checked ? "No laptop banners" : "Laptop banners on"
                            onActivated: Shell.dock.setPreference("dnd", !checked)
                        }
                        ToggleTile {
                            width: tiles.tileWidth
                            dense: root.landscape
                            icon: Shell.rotationMode === "auto" ? "rotate-auto" : "rotate"
                            label: "Rotation"
                            // Dock Mode is always landscape; the preference applies again after it.
                            supported: Shell.output.available && !Shell.docked
                            busy: Shell.output.busy
                            checked: Shell.rotationMode !== "portrait"
                            detail: Shell.docked ? "Landscape in Dock"
                                  : Shell.rotationMode === "auto"
                                    ? (Shell.sensor.available ? "Auto (" + (Shell.sensor.claimed ? Shell.sensor.orientation : "no sensor access") + ")"
                                                              : "Auto: no sensor")
                                    : Shell.rotationMode === "landscape" ? "Landscape" : "Portrait"
                            // portrait → landscape → auto (only offered when a sensor exists) → portrait
                            onActivated: Shell.setRotationMode(
                                Shell.rotationMode === "portrait" ? "landscape"
                                : Shell.rotationMode === "landscape" && Shell.sensor.available ? "auto" : "portrait")
                        }
                        ToggleTile {
                            width: tiles.tileWidth
                            dense: root.landscape
                            icon: "keyboard"
                            label: "Keyboard"
                            supported: Shell.osk.available
                            checked: Shell.osk.visible
                            detail: Shell.osk.available ? (Shell.osk.visible ? "Shown" : "Shows on text input") : "Not running"
                            onActivated: Shell.osk.toggle()
                        }
                        ToggleTile {
                            width: tiles.tileWidth
                            dense: root.landscape
                            visible: Shell.dock.deckEditable
                            icon: "motion"
                            label: "Reduce motion"
                            checked: Shell.reduceMotion
                            detail: checked ? "Minimal animation" : "Off"
                            onActivated: Shell.dock.setPreference("reduce_motion", !checked)
                        }
                        ToggleTile {
                            width: tiles.tileWidth
                            dense: root.landscape
                            visible: !root.landscape
                            icon: "power"
                            label: "Screen off"
                            momentary: true
                            supported: Shell.display.available
                            detail: Shell.display.available ? "Power key wakes" : "Not supported"
                            onActivated: Shell.setDisplayOn(false)
                        }
                        ToggleTile {
                            width: tiles.tileWidth
                            dense: root.landscape
                            visible: !root.landscape
                            icon: "code"
                            label: "Developer"
                            momentary: true
                            detail: "Core: " + Shell.core.statusText.toLowerCase()
                            onActivated: Shell.panel = "dev"
                        }
                    }
                }
            }
        }

        // Scroll hint: a fade at the bottom edge while more content is below.
        Rectangle {
            anchors.left: parent.left
            anchors.right: parent.right
            anchors.bottom: parent.bottom
            anchors.margins: 1
            height: 24
            radius: Theme.radiusLarge
            visible: flick.interactive && !flick.atYEnd
            gradient: Gradient {
                GradientStop { position: 0; color: "transparent" }
                GradientStop { position: 1; color: Theme.surface }
            }
        }
    }
}
