import QtQuick
import "navmodel.js" as Nav

// Dock navigation (UI-owned, transient; docs/dock-mode/ux-specification.md). The rules live
// in navmodel.js (pure, unit-tested); this object only feeds it events with the current
// time and arms ONE single-shot timer for the next deadline. No periodic wakeups: with
// Home showing, a pin, a modal or a 0 timeout, no timer runs at all.
QtObject {
    id: nav

    property var s: Nav.initial(Nav.DEFAULT_IDLE_MS, Date.now())

    readonly property string view: s.view
    readonly property string page: s.page
    readonly property bool pinned: s.pinned
    readonly property string modal: s.modal
    readonly property string globalInteraction: s.global
    readonly property int idleTimeoutMs: s.idleTimeoutMs
    // Wall-clock time of the pending idle return (-1 = none); for diagnostics/tests.
    readonly property real deadline: Nav.nextDeadline(s)

    signal changed(string what)

    function dispatch(ev) {
        ev.now = Date.now()
        const before = s.view
        s = Nav.reduce(s, ev)
        _arm()
        if (s.view !== before)
            changed(ev.type === "tick" ? "idle" : ev.type)
    }
    function navigate(view) { dispatch({ type: "navigate", view: view }) }
    function selectPage(page) { dispatch({ type: "page", page: page }) }
    function interact() { dispatch({ type: "interact" }) }
    function setPinned(on) { dispatch({ type: "pin", pinned: on }) }
    function openModal(name) { dispatch({ type: "modal", name: name }) }
    function closeModal() { if (s.modal !== "") dispatch({ type: "modal", name: "" }) }
    function setGlobal(name) { dispatch({ type: "global", name: name }) }
    function setIdleTimeout(ms) { dispatch({ type: "timeout", idleTimeoutMs: ms }) }
    function reset() { dispatch({ type: "reset" }) }

    function _arm() {
        const d = Nav.nextDeadline(s)
        if (d < 0) {
            _timer.stop()
            return
        }
        _timer.interval = Math.max(1, d - Date.now() + 5)
        _timer.restart()
    }

    readonly property Timer _timer: Timer {
        repeat: false
        onTriggered: nav.dispatch({ type: "tick" })
    }
}
