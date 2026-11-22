import QtQuick
import Pocket.Core
import "../../components"
import "../../theme"

// Stage 1 development panel: shows the live link to Pocket Core and switches the logical
// mode through it. The displayed mode is always the core's (from GetState/StateChanged),
// never a local guess. Runtime-agnostic: Qt Quick + the Pocket.Core QtDBus module.
Rectangle {
    id: root

    property string runtimeName: "unknown"
    property string lastEvent: "none yet"
    readonly property bool landscape: width > height
    readonly property bool connected: core.status === CoreClient.Connected
    readonly property string otherMode: core.mode === "dock" ? "handheld" : "dock"

    color: Theme.background

    function title(s) { return s.length ? s.charAt(0).toUpperCase() + s.slice(1) : "—" }

    LoggingCategory {
        id: diag
        name: "pocket.shell.diag"
        defaultLogLevel: LoggingCategory.Info
    }

    function emitDiagnostic(name, fields) {
        // One machine-greppable line per event: "pocket.shell.diag: event=<name> k=v ..."
        let line = "event=" + name
        for (const k in fields)
            line += " " + k + "=" + fields[k]
        console.info(diag, line)
        lastEvent = line
    }

    CoreClient {
        id: core
        onStatusChanged: root.emitDiagnostic("core.status", {
            status: statusText.toLowerCase(), owner: owner || "-", connections: connections,
            error: lastError ? JSON.stringify(lastError) : "-" })
        onModeChanged: root.emitDiagnostic("core.mode", { mode: mode, revision: revision })
        onRequestFinished: (mode, ok, error) => root.emitDiagnostic("core.request", {
            mode: mode, ok: ok, error: ok ? "-" : JSON.stringify(error) })
    }

    // The surface is configured asynchronously; report layout only once it has a real size.
    // Sizes arrive one dimension at a time: defer until the configure settles, and report
    // only actual changes.
    property var reportedLandscape: undefined
    function reportLayout() {
        if (width <= 0 || height <= 0 || landscape === reportedLandscape)
            return
        const first = reportedLandscape === undefined
        reportedLandscape = landscape
        emitDiagnostic(first ? "shell.started" : "layout.orientation",
                       { runtime: runtimeName, size: width + "x" + height, landscape: landscape })
    }
    onWidthChanged: Qt.callLater(reportLayout)
    onHeightChanged: Qt.callLater(reportLayout)

    Grid {
        anchors.centerIn: parent
        columns: root.landscape ? 2 : 1
        spacing: Theme.spacing * 2
        horizontalItemAlignment: Grid.AlignHCenter
        verticalItemAlignment: Grid.AlignVCenter

        Column {
            spacing: Theme.spacing / 2
            Text {
                anchors.horizontalCenter: parent.horizontalCenter
                text: "Pocket OS"
                color: Theme.text
                font.pixelSize: Theme.fontTitle
                font.weight: Font.DemiBold
            }
            Row {
                anchors.horizontalCenter: parent.horizontalCenter
                spacing: Theme.spacing / 2
                Rectangle {
                    anchors.verticalCenter: parent.verticalCenter
                    width: 12; height: 12; radius: 6
                    color: core.status === CoreClient.Connected ? Theme.accent
                         : core.status === CoreClient.Connecting ? Theme.warning : Theme.danger
                }
                Text {
                    text: "Core: " + core.statusText
                    color: Theme.text
                    font.pixelSize: Theme.fontBody + 2
                }
            }
            Text {
                anchors.horizontalCenter: parent.horizontalCenter
                text: "Mode: " + root.title(core.mode) + (root.connected ? "" : " (last known)")
                color: root.connected ? Theme.text : Theme.textMuted
                font.pixelSize: Theme.fontBody + 8
                font.weight: Font.DemiBold
            }
        }

        Column {
            spacing: Theme.spacing
            PocketButton {
                anchors.horizontalCenter: parent.horizontalCenter
                enabled: root.connected && !core.requestPending
                text: "Switch to " + root.title(root.otherMode)
                checked: core.mode === "dock"
                onActivated: (point) => {
                    const target = root.otherMode
                    root.emitDiagnostic("button.activated", {
                        request: target,
                        x: point.position.x.toFixed(0),
                        y: point.position.y.toFixed(0),
                        device: point.device ? point.device.name : "unknown"
                    })
                    core.requestMode(target)
                }
            }
            Text {
                anchors.horizontalCenter: parent.horizontalCenter
                text: root.connected
                      ? "pocketd " + core.coreVersion + " · protocol " + core.protocol
                        + " · rev " + core.revision + " · " + core.owner
                      : (core.lastError || "waiting for pocketd")
                color: Theme.textMuted
                font.pixelSize: Theme.fontSmall
                width: Math.min(root.width - 2 * Theme.spacing, 340)
                horizontalAlignment: Text.AlignHCenter
                wrapMode: Text.Wrap
            }
            Text {
                anchors.horizontalCenter: parent.horizontalCenter
                text: root.runtimeName + " · " + root.width + "×" + root.height
                      + (root.landscape ? " · landscape" : " · portrait")
                      + " · links " + core.connections
                color: Theme.textMuted
                font.pixelSize: Theme.fontSmall
            }
            Text {
                width: Math.min(root.width - 2 * Theme.spacing, 340)
                horizontalAlignment: Text.AlignHCenter
                wrapMode: Text.WrapAnywhere
                text: "last: " + root.lastEvent
                color: Theme.textMuted
                font.pixelSize: Theme.fontSmall - 2
            }
        }
    }
}
