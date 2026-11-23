import QtQuick
import "../../state"
import "../../theme"

// One Control Deck tile. Every state comes from Core or the laptop, never a local guess:
//   unavailable  not connected / not offered / not granted / unavailable (dimmed, reason)
//   pending      Core accepted the request; waiting for the laptop (spinner)
//   progress     workflow step reports
//   result       success (fades after 3 s), error (stays), unknown outcome (tap = Retry)
Rectangle {
    id: root
    property var tile                 // {id, action, label, icon, params}
    property var action: null         // the laptop's advertisement, or null
    property bool connected: false
    property bool pending: false
    property var result: null         // Shell.tileResults[tile.id]
    property bool showResult: false

    readonly property bool ready: connected && action !== null && action.available
    readonly property bool uncertain: result !== null && result.uncertain === true
    readonly property bool tappable: ready && !pending
    readonly property string status: {
        if (pending)
            return result && result.progress ? result.text : "Waiting for the laptop…"
        if (!connected)
            return "Not connected"
        if (action === null)
            return "Not offered by " + (Shell.dock.hostName || "the laptop")
        if (!action.available)
            return action.privilege === "elevated" && action.reason.indexOf("not granted") === 0
                ? "Not granted on " + (Shell.dock.hostName || "the laptop")
                : (action.reason || "Unavailable")
        if (uncertain)
            return "Outcome unknown · tap to retry"
        if (result && (showResult || !result.ok))
            return (result.ok ? "✓ " : "✕ ") + result.text
        return ""
    }
    readonly property color statusColor: pending || !ready ? Theme.textMuted
        : uncertain ? Theme.warning
        : result && (showResult || !result.ok) ? (result.ok ? Theme.success : Theme.danger) : Theme.textMuted

    onResultChanged: {
        showResult = result !== null && result.ok && !result.progress
        if (showResult)
            fade.restart()
    }
    Timer { id: fade; interval: 3000; onTriggered: root.showResult = false }

    radius: Theme.radius
    color: !tappable ? Theme.surface : tap.pressed ? Theme.surfacePressed : Theme.surfaceRaised
    border.width: 1
    border.color: !ready ? Theme.outline
        : uncertain ? Theme.warning
        : result && !result.ok ? Theme.danger : "transparent"
    scale: tap.pressed ? 0.97 : 1
    Behavior on scale { NumberAnimation { duration: Theme.durationFast } }
    Accessible.role: Accessible.Button
    Accessible.name: tile ? tile.label : ""

    Image {
        id: icon
        x: Theme.space3
        y: Theme.space3
        width: Theme.icon; height: Theme.icon
        sourceSize: Qt.size(Theme.icon * 2, Theme.icon * 2)
        source: Theme.iconUrl(root.tile && root.tile.icon ? root.tile.icon : "action")
        onStatusChanged: if (status === Image.Error) source = Theme.iconUrl("action")
        opacity: root.ready ? 1 : 0.35
    }
    // Busy indicator: a small arc, animated on the render thread only while pending.
    Item {
        anchors.right: parent.right
        anchors.top: parent.top
        anchors.margins: Theme.space3
        width: 16; height: 16
        visible: root.pending
        Rectangle {
            anchors.fill: parent
            radius: 8
            color: "transparent"
            border.color: Theme.accent
            border.width: 2
            Rectangle { width: 6; height: 6; radius: 3; color: Theme.surface; x: 10; y: 0 }
        }
        RotationAnimator on rotation {
            from: 0; to: 360; duration: 900
            loops: Animation.Infinite
            running: root.pending && root.visible
        }
    }
    Column {
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.bottom: parent.bottom
        anchors.margins: Theme.space3
        spacing: 1
        Text {
            width: parent.width
            text: root.tile ? root.tile.label : ""
            color: root.ready ? Theme.text : Theme.textDisabled
            font.pixelSize: Theme.fontBody
            font.weight: Font.DemiBold
            elide: Text.ElideRight
        }
        Text {
            width: parent.width
            visible: text.length > 0
            text: root.status
            color: root.statusColor
            font.pixelSize: Theme.fontCaption
            elide: Text.ElideRight
        }
    }

    TapHandler {
        id: tap
        enabled: root.tappable
        gesturePolicy: TapHandler.ReleaseWithinBounds
        onTapped: root.uncertain ? Shell.retryDeckTile(root.tile) : Shell.runDeckTile(root.tile)
    }
}
