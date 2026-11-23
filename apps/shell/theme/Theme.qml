pragma Singleton
import QtQuick

// Pocket design tokens (docs/development/ui.md). Plain Qt Quick singleton, so it works
// under any QML host. All sizes are logical pixels: Phoc applies the output scale
// (3 on lavender), never multiply by the physical resolution here.
QtObject {
    // ── Color ──────────────────────────────────────────────────────────────────
    // One dark theme. Surfaces get lighter as they rise; accent marks "on"/primary.
    readonly property color background: "#0e1116"      // home backdrop
    readonly property color surface: "#171b22"         // panels, bars
    readonly property color surfaceRaised: "#222833"   // tiles, buttons at rest
    readonly property color surfacePressed: "#2e3643"  // pressed feedback
    readonly property color outline: "#2f3744"
    readonly property color scrim: "#a6000000"         // behind modal panels
    readonly property color accent: "#4c8dff"          // active toggles, primary
    readonly property color accentText: "#ffffff"
    readonly property color text: "#e8edf4"
    readonly property color textMuted: "#98a2b3"
    readonly property color textDisabled: "#5c6676"
    readonly property color success: "#3fb950"
    readonly property color warning: "#d29922"
    readonly property color danger: "#f85149"

    // ── Typography (Droid Sans on the device) ─────────────────────────────────
    readonly property int fontDisplay: 56   // home clock
    readonly property int fontTitle: 20
    readonly property int fontBody: 15
    readonly property int fontLabel: 13
    readonly property int fontCaption: 11

    // ── Spacing scale ─────────────────────────────────────────────────────────
    readonly property int space1: 4
    readonly property int space2: 8
    readonly property int space3: 12
    readonly property int space4: 16
    readonly property int space5: 24
    readonly property int space6: 32
    readonly property int spacing: space4   // default gap (Stage 1 name)

    // ── Shape ─────────────────────────────────────────────────────────────────
    readonly property int radiusSmall: 8
    readonly property int radius: 14
    readonly property int radiusLarge: 22

    // ── Touch and sizes ───────────────────────────────────────────────────────
    readonly property int touchTarget: 44   // minimum hit size (≈8 mm at scale 3, 409 ppi)
    readonly property int iconSmall: 16
    readonly property int icon: 22
    readonly property int appIcon: 52
    readonly property int statusBarHeight: 28
    readonly property int navBarHeight: 44

    // ── Motion ────────────────────────────────────────────────────────────────
    // Short, single-property animations only (opacity/scale/position): the Adreno 512
    // handles these cheaply. No continuous animations while idle.
    readonly property int durationFast: 110
    readonly property int duration: 180
    readonly property int durationSlow: 260
    readonly property int easing: Easing.OutCubic

    // Monochrome UI icons (white, 24-unit grid) shipped in theme/icons/<name>.svg.
    function iconUrl(name) { return Qt.resolvedUrl("icons/" + name + ".svg") }
}
