pragma Singleton
import QtQuick

// Pocket design tokens (docs/dock-mode/design-system.md). Plain Qt Quick singleton, so it
// works under any QML host. All sizes are logical pixels: Phoc applies the output scale
// (3 on lavender), never multiply by the physical resolution here.
//
// One dark theme for a phone that sits on a desk next to a laptop: deep neutral
// backgrounds, surfaces that get lighter as they rise, one blue accent for "on/primary",
// and soft tinted wells for categories and states. No shadows (cheap to render on the
// Adreno 512); depth comes from surface steps and 1 px outlines.
QtObject {
    id: theme

    // ── Color ──────────────────────────────────────────────────────────────────
    readonly property color background: "#0b0e13"     // backdrop (home, dock)
    readonly property color surface: "#141922"        // panels, sheets, rail
    readonly property color surfaceRaised: "#1c222d"  // cards, tiles, buttons at rest
    readonly property color surfaceHigh: "#262d3a"    // selected/raised controls
    readonly property color surfacePressed: "#303848" // pressed feedback
    readonly property color outline: "#2a3240"
    readonly property color outlineStrong: "#3a4456"
    readonly property color scrim: "#b3000000"        // behind modal panels
    readonly property color accent: "#5b96ff"         // active toggles, primary
    readonly property color accentSoft: "#2a5b96ff"   // tinted wells, selected chips
    readonly property color accentText: "#ffffff"
    readonly property color text: "#eaeef5"
    readonly property color textMuted: "#9aa4b5"
    readonly property color textDisabled: "#5d6778"
    readonly property color success: "#46c46a"
    readonly property color successSoft: "#2646c46a"
    readonly property color warning: "#e0a63a"
    readonly property color warningSoft: "#26e0a63a"
    readonly property color danger: "#ff6b62"
    readonly property color dangerSoft: "#26ff6b62"

    // Category hues (Deck tiles, notification icons). Desaturated so a full deck stays calm.
    function categoryColor(category) {
        switch (category) {
        case "apps": return "#9b8cff"
        case "desktop": return "#5b96ff"
        case "media": return "#ff7eb3"
        case "session": return "#e0a63a"
        case "workflow": return "#3fc7b4"
        case "access": return "#9aa4b5"
        default: return "#7fb0ff"
        }
    }
    function soft(c, alpha) { return Qt.rgba(c.r, c.g, c.b, alpha === undefined ? 0.16 : alpha) }

    // ── Typography (Droid Sans on the device) ─────────────────────────────────
    readonly property int fontHero: 120     // Ambient Home clock
    readonly property int fontDisplay: 56   // handheld home clock
    readonly property int fontHeadline: 24
    readonly property int fontTitle: 19
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
    readonly property int gutter: space4    // screen edge margin

    // ── Shape ─────────────────────────────────────────────────────────────────
    readonly property int radiusSmall: 10
    readonly property int radius: 16
    readonly property int radiusLarge: 24

    // ── Touch and sizes ───────────────────────────────────────────────────────
    readonly property int touchTarget: 44   // minimum hit size (≈8 mm at scale 3, 409 ppi)
    readonly property int touchLarge: 52    // primary controls on the Dock
    readonly property int iconSmall: 16
    readonly property int icon: 22
    readonly property int iconLarge: 28
    readonly property int appIcon: 52
    readonly property int statusBarHeight: 28
    readonly property int navBarHeight: 44
    readonly property int railWidth: 68

    // ── Motion ────────────────────────────────────────────────────────────────
    // Short, transform/opacity-only animations (cheap on the Adreno 512). Nothing runs
    // while idle; spinners run only while visible and busy. `reduceMotion` (a Dock
    // preference kept by Pocket Core) turns movement into instant changes.
    property bool reduceMotion: false
    readonly property int durationFast: reduceMotion ? 0 : 110
    readonly property int duration: reduceMotion ? 0 : 200
    readonly property int durationSlow: reduceMotion ? 0 : 280
    readonly property int easing: Easing.OutCubic
    readonly property int easingEnter: Easing.OutQuart
    readonly property int easingExit: Easing.InCubic
    // Slide distance of view and sheet transitions.
    readonly property int travel: reduceMotion ? 0 : 28

    // Monochrome UI icons (white, 24-unit grid) shipped in theme/icons/<name>.svg.
    function iconUrl(name) { return Qt.resolvedUrl("icons/" + name + ".svg") }
    // Icons a Deck tile may use (the editor's picker).
    readonly property var tileIcons: ["action", "apps", "bell", "code", "desktop", "folder", "globe",
        "grid", "home", "keyboard", "laptop", "lock", "music", "pause", "play", "play-pause",
        "power", "search", "skip-back", "skip-forward", "sun", "terminal", "volume", "volume-down",
        "volume-off", "windows", "chevron-left", "chevron-right", "workflow", "star", "edit"]
}
