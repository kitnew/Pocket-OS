import QtQuick
import "../../components"
import "../../theme"

// Stage 0 prototype content: identity, live layout info, and one interactive control
// whose activation changes visible state and emits a diagnostic event.
// Runtime-agnostic: depends only on Qt Quick.
Rectangle {
    id: root

    property string runtimeName: "unknown"
    property int activations: 0
    property string lastEvent: "none yet"
    readonly property bool landscape: width > height

    color: Theme.background

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

    // The surface is configured asynchronously; report layout only once it has a real size.
    // Sizes arrive one dimension at a time: defer reporting until the configure settles,
    // and report only actual changes.
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
            Text {
                anchors.horizontalCenter: parent.horizontalCenter
                text: "Stage 0 shell prototype"
                color: Theme.textMuted
                font.pixelSize: Theme.fontBody
            }
        }

        Column {
            spacing: Theme.spacing
            PocketButton {
                anchors.horizontalCenter: parent.horizontalCenter
                text: root.activations === 0 ? "Tap to test" : "Tapped ×" + root.activations
                checked: root.activations % 2 === 1
                onActivated: (point) => {
                    root.activations++
                    root.emitDiagnostic("button.activated", {
                        count: root.activations,
                        x: point.position.x.toFixed(0),
                        y: point.position.y.toFixed(0),
                        device: point.device ? point.device.name : "unknown"
                    })
                }
            }
            Text {
                anchors.horizontalCenter: parent.horizontalCenter
                text: root.runtimeName + " · " + root.width + "×" + root.height
                      + (root.landscape ? " · landscape" : " · portrait")
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
