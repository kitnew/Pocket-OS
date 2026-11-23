import QtQuick
import "../../components"
import "../../state"
import "../../theme"

// Quick Settings: real system controls and states. A tile is interactive only when the
// control actually works on this system; otherwise it is dimmed with the reason.
// Tapping outside the card closes the panel.
Item {
    id: root
    readonly property var sys: Shell.system
    readonly property bool landscape: width > height

    Rectangle {
        anchors.fill: parent
        color: Theme.scrim
        TapHandler { onTapped: Shell.closePanel() }
    }

    PanelContainer {
        id: card
        anchors.top: parent.top
        anchors.horizontalCenter: parent.horizontalCenter
        anchors.topMargin: Theme.statusBarHeight + Theme.space2
        width: Math.min(parent.width - 2 * Theme.space3, 560)
        height: body.implicitHeight + 2 * padding
        // Swallow taps on the card itself (only the scrim closes).
        TapHandler {}

        Column {
            id: body
            width: card.width - 2 * card.padding
            spacing: Theme.space4

            Row {
                width: parent.width
                Text {
                    width: parent.width / 2
                    text: Qt.formatDate(Shell.now, "ddd d MMM") + "  " + Qt.formatTime(Shell.now, "HH:mm")
                    color: Theme.text
                    font.pixelSize: Theme.fontBody
                    font.weight: Font.DemiBold
                }
                Text {
                    width: parent.width / 2
                    horizontalAlignment: Text.AlignRight
                    text: !root.sys || !root.sys.batteryAvailable ? "No battery info"
                        : Math.round(root.sys.batteryPercent) + "% · "
                          + (root.sys.batteryFull ? "full" : root.sys.batteryCharging ? "charging" : "on battery")
                    color: Theme.textMuted
                    font.pixelSize: Theme.fontLabel
                }
            }

            PocketSlider {
                width: parent.width
                icon: "sun"
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

            // Volume of the default output (PipeWire). The mute button and slider act on the
            // real sink; external changes (wpctl, apps, device switches) show up here.
            Row {
                id: volumeRow
                width: parent.width
                spacing: Theme.space2
                readonly property var audio: Shell.audio
                readonly property bool usable: audio !== null && audio.available
                IconButton {
                    icon: volumeRow.usable && !volumeRow.audio.muted && volumeRow.audio.volume > 0 ? "volume" : "volume-off"
                    accessibleName: volumeRow.usable && volumeRow.audio.muted ? "Unmute" : "Mute"
                    enabled: volumeRow.usable
                    checked: volumeRow.usable && volumeRow.audio.muted
                    flat: false
                    onActivated: Shell.setMuted(!volumeRow.audio.muted)
                }
                PocketSlider {
                    width: parent.width - Theme.touchTarget - parent.spacing
                    anchors.verticalCenter: parent.verticalCenter
                    supported: volumeRow.usable
                    value: volumeRow.usable ? volumeRow.audio.volume : 0
                    onMoved: (v) => Shell.setVolume(v)
                }
            }
            Text {
                width: parent.width
                wrapMode: Text.Wrap
                text: Shell.volumeStatus
                color: Shell.audio && Shell.audio.available ? Theme.textMuted : Theme.danger
                font.pixelSize: Theme.fontCaption
            }

            Grid {
                id: tiles
                width: parent.width
                columns: root.landscape ? 4 : 2
                spacing: Theme.space2
                readonly property real tileWidth: (width - (columns - 1) * spacing) / columns

                ToggleTile {
                    width: tiles.tileWidth
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
                    icon: "dock"
                    label: "Dock mode"
                    supported: Shell.coreConnected
                    busy: Shell.core.requestPending
                    checked: Shell.docked
                    detail: !Shell.coreConnected ? "Core not connected"
                          : !Shell.dock.supported ? (Shell.docked ? "On" : "Off")
                          : Shell.dock.state === "connected" ? (Shell.docked ? "On · " : "Off · ") + Shell.dock.hostName
                          : Shell.docked ? "On · no computer" : "Off"
                    onActivated: Shell.requestMode(Shell.docked ? "handheld" : "dock")
                }
                ToggleTile {
                    width: tiles.tileWidth
                    icon: Shell.rotationMode === "auto" ? "rotate-auto" : "rotate"
                    label: "Rotation"
                    // Dock Mode is always landscape; the preference applies again after it.
                    supported: Shell.output.available && !Shell.docked
                    busy: Shell.output.busy
                    checked: Shell.rotationMode !== "portrait"
                    detail: Shell.docked ? "Landscape in Dock mode"
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
                    icon: "keyboard"
                    label: "Keyboard"
                    supported: Shell.osk.available
                    checked: Shell.osk.visible
                    detail: Shell.osk.available ? (Shell.osk.visible ? "Shown" : "Shows on text input") : "Not running"
                    onActivated: Shell.osk.toggle()
                }
                ToggleTile {
                    width: tiles.tileWidth
                    icon: "power"
                    label: "Screen off"
                    supported: Shell.display.available
                    detail: Shell.display.available ? "Power key wakes" : "Not supported"
                    onActivated: Shell.setDisplayOn(false)
                }
                ToggleTile {
                    width: tiles.tileWidth
                    icon: "code"
                    label: "Developer"
                    detail: "Core: " + Shell.core.statusText.toLowerCase()
                    onActivated: Shell.panel = "dev"
                }
            }
        }
    }
}
