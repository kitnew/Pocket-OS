.pragma library
// Dock navigation model (docs/dock-mode/ux-specification.md, "Navigation state machine").
//
// A pure reducer: no timers, no QML, no globals. state/DockNav.qml drives it with one
// single-shot timer armed for nextDeadline(); tests/shell/navmodel.test.mjs runs it under
// node. Time is passed in (`now`, ms) by the caller, so every transition is deterministic.
//
// State: { view, page, pinned, lastInteraction, modal, global, idleTimeoutMs }
//   view      "home" | "deck" | "link"       (Base View)
//   page      selected Control Deck page id ("" = first page)
//   pinned    no automatic return to Home while set
//   modal     name of an open modal Context Overlay ("" = none); suspends idle return
//   global    name of an active Global Interaction ("" = none); suspends idle return
//   idleTimeoutMs  0 = never return automatically

var VIEWS = ["home", "deck", "link"]
var DEFAULT_IDLE_MS = 15000

function initial(idleTimeoutMs, now) {
    return {
        view: "home",
        page: "",
        pinned: false,
        lastInteraction: now,
        modal: "",
        global: "",
        idleTimeoutMs: idleTimeoutMs === undefined ? DEFAULT_IDLE_MS : idleTimeoutMs
    }
}

function copy(s) {
    return {
        view: s.view, page: s.page, pinned: s.pinned, lastInteraction: s.lastInteraction,
        modal: s.modal, global: s.global, idleTimeoutMs: s.idleTimeoutMs
    }
}

// Idle return is possible only when all of these hold.
function idleArmed(s) {
    return s.view !== "home" && !s.pinned && s.modal === "" && s.global === "" && s.idleTimeoutMs > 0
}

// Absolute time (ms) at which a "tick" event changes the state, or -1 when nothing is due.
function nextDeadline(s) {
    return idleArmed(s) ? s.lastInteraction + s.idleTimeoutMs : -1
}

// Returns a new state; never mutates `s`. Unknown events and invalid arguments leave the
// state unchanged (apart from bookkeeping), so a bad caller cannot break navigation.
function reduce(s, ev) {
    var n = copy(s)
    var now = ev.now
    // A wall clock that went backwards must not postpone idle return indefinitely.
    if (now !== undefined && now < n.lastInteraction)
        n.lastInteraction = now
    switch (ev.type) {
    case "navigate":
        if (VIEWS.indexOf(ev.view) < 0)
            return n
        n.view = ev.view
        if (ev.page !== undefined)
            n.page = ev.page
        n.lastInteraction = now
        return n
    case "page":
        n.page = ev.page
        n.lastInteraction = now
        return n
    case "interact":
        n.lastInteraction = now
        return n
    case "pin":
        n.pinned = !!ev.pinned
        n.lastInteraction = now
        return n
    case "modal":
        // Opening counts as interaction; closing restarts the idle period from now.
        n.modal = ev.name || ""
        n.lastInteraction = now
        return n
    case "global":
        n.global = ev.name || ""
        n.lastInteraction = now
        return n
    case "timeout":
        n.idleTimeoutMs = Math.max(0, ev.idleTimeoutMs | 0)
        return n
    case "reset":
        // Entering Dock Mode: Home, unpinned, nothing open.
        return initial(n.idleTimeoutMs, now)
    case "tick": {
        var d = nextDeadline(n)
        if (d >= 0 && now >= d) {
            n.view = "home"
            n.lastInteraction = now
        }
        return n
    }
    default:
        return n
    }
}
