pragma Singleton
import QtQuick

// Design tokens. Plain Qt Quick singleton (registered in qmldir) so it works under
// any QML runtime. Sizes are logical pixels (the compositor applies output scale).
QtObject {
    readonly property color background: "#0f1216"
    readonly property color surface: "#1b2027"
    readonly property color surfaceActive: "#2f81f7"
    readonly property color accent: "#3fb950"
    readonly property color text: "#e6edf3"
    readonly property color textMuted: "#8b949e"

    readonly property int spacing: 16
    readonly property int radius: 16
    readonly property int touchTarget: 56   // minimum comfortable touch target

    readonly property int fontTitle: 34
    readonly property int fontBody: 18
    readonly property int fontSmall: 14
}
