// Unit tests for the Dock navigation model (apps/shell/state/navmodel.js).
//   node --test tests/shell/        (part of scripts/check.sh)
// The model is a QML JavaScript library; it is loaded here without QML by dropping the
// `.pragma library` line and evaluating the plain functions.
import { test } from "node:test"
import assert from "node:assert/strict"
import { readFileSync } from "node:fs"

const src = readFileSync(new URL("../../apps/shell/state/navmodel.js", import.meta.url), "utf8")
    .replace(/^\.pragma library\s*$/m, "")
const Nav = new Function(src + "\nreturn { initial, reduce, nextDeadline, idleArmed, VIEWS, DEFAULT_IDLE_MS }")()

const T = 15000
const run = (s, ...events) => events.reduce((acc, e) => Nav.reduce(acc, e), s)
const start = () => Nav.initial(T, 0)

test("starts at Home, unpinned, with the default 15 s timeout", () => {
    const s = Nav.initial(undefined, 0)
    assert.equal(s.view, "home")
    assert.equal(s.pinned, false)
    assert.equal(s.idleTimeoutMs, 15000)
    assert.equal(Nav.nextDeadline(s), -1, "Home never needs a timer")
})

test("manual navigation between views", () => {
    let s = run(start(), { type: "navigate", view: "deck", now: 100 })
    assert.equal(s.view, "deck")
    s = run(s, { type: "navigate", view: "link", now: 200 })
    assert.equal(s.view, "link")
    s = run(s, { type: "navigate", view: "home", now: 300 })
    assert.equal(s.view, "home")
})

test("unknown views and events leave the state alone", () => {
    const s0 = run(start(), { type: "navigate", view: "deck", now: 100 })
    assert.equal(run(s0, { type: "navigate", view: "settings", now: 200 }).view, "deck")
    assert.equal(run(s0, { type: "bogus", now: 200 }).view, "deck")
})

test("idle return to Home exactly at the timeout", () => {
    const s = run(start(), { type: "navigate", view: "deck", now: 1000 })
    assert.equal(Nav.nextDeadline(s), 1000 + T)
    assert.equal(run(s, { type: "tick", now: 1000 + T - 1 }).view, "deck", "not before")
    assert.equal(run(s, { type: "tick", now: 1000 + T }).view, "home", "at the deadline")
})

test("touch resets the idle timer", () => {
    let s = run(start(), { type: "navigate", view: "deck", now: 0 }, { type: "interact", now: 10000 })
    assert.equal(Nav.nextDeadline(s), 10000 + T)
    assert.equal(run(s, { type: "tick", now: T }).view, "deck")
    assert.equal(run(s, { type: "tick", now: 10000 + T }).view, "home")
})

test("pinning prevents idle return and survives navigation", () => {
    let s = run(start(), { type: "navigate", view: "deck", now: 0 }, { type: "pin", pinned: true, now: 0 })
    assert.equal(Nav.nextDeadline(s), -1)
    assert.equal(run(s, { type: "tick", now: 10 * T }).view, "deck")
    s = run(s, { type: "navigate", view: "link", now: 5 })
    assert.equal(s.pinned, true, "the pin moves with the user")
    assert.equal(run(s, { type: "tick", now: 10 * T }).view, "link")
    s = run(s, { type: "pin", pinned: false, now: 100 })
    assert.equal(Nav.nextDeadline(s), 100 + T)
    assert.equal(run(s, { type: "tick", now: 100 + T }).view, "home")
})

test("an open modal suspends idle return; closing restarts the period", () => {
    let s = run(start(), { type: "navigate", view: "deck", now: 0 }, { type: "modal", name: "confirm", now: 1000 })
    assert.equal(Nav.nextDeadline(s), -1)
    assert.equal(run(s, { type: "tick", now: 60000 }).view, "deck")
    s = run(s, { type: "modal", name: "", now: 60000 })
    assert.equal(Nav.nextDeadline(s), 60000 + T)
    assert.equal(run(s, { type: "tick", now: 60000 + T - 1 }).view, "deck")
    assert.equal(run(s, { type: "tick", now: 60000 + T }).view, "home")
})

test("Global Interaction (future voice) suspends idle return", () => {
    const s = run(start(), { type: "navigate", view: "deck", now: 0 }, { type: "global", name: "voice", now: 0 })
    assert.equal(Nav.idleArmed(s), false)
    assert.equal(run(s, { type: "tick", now: 10 * T }).view, "deck")
})

test("overlays and idle return keep the selected deck page", () => {
    let s = run(start(), { type: "navigate", view: "deck", page: "media", now: 0 },
        { type: "modal", name: "confirm", now: 1 }, { type: "modal", name: "", now: 2 })
    assert.equal(s.page, "media")
    s = run(s, { type: "tick", now: 2 + T })
    assert.equal(s.view, "home")
    assert.equal(s.page, "media", "coming back to the deck shows the same page")
    s = run(s, { type: "page", page: "general", now: 3 + T })
    assert.equal(s.page, "general")
})

test("configurable timeout; 0 never returns automatically", () => {
    let s = run(start(), { type: "timeout", idleTimeoutMs: 5000 }, { type: "navigate", view: "deck", now: 0 })
    assert.equal(Nav.nextDeadline(s), 5000)
    s = run(s, { type: "timeout", idleTimeoutMs: 0 })
    assert.equal(Nav.nextDeadline(s), -1)
    assert.equal(run(s, { type: "tick", now: 1e9 }).view, "deck")
})

test("reset (entering Dock Mode) goes Home, unpinned, closes overlays, keeps the timeout", () => {
    const s = run(start(), { type: "timeout", idleTimeoutMs: 30000 },
        { type: "navigate", view: "link", now: 0 }, { type: "pin", pinned: true, now: 0 },
        { type: "modal", name: "confirm", now: 0 }, { type: "reset", now: 50 })
    assert.deepEqual([s.view, s.pinned, s.modal, s.idleTimeoutMs], ["home", false, "", 30000])
})

test("a clock that jumps backwards does not freeze idle return", () => {
    let s = run(start(), { type: "navigate", view: "deck", now: 1_000_000 })
    s = run(s, { type: "tick", now: 10 })   // wall clock reset (no RTC on lavender)
    assert.equal(s.view, "deck")
    assert.equal(Nav.nextDeadline(s), 10 + T)
    assert.equal(run(s, { type: "tick", now: 10 + T }).view, "home")
})

test("the reducer never mutates its input", () => {
    const s = start()
    const frozen = JSON.stringify(s)
    Nav.reduce(s, { type: "navigate", view: "deck", now: 5 })
    Nav.reduce(s, { type: "pin", pinned: true, now: 5 })
    assert.equal(JSON.stringify(s), frozen)
})

test("informational sheets (details, notifications) do not pin the view; idle closes them", () => {
    let s = run(start(), { type: "navigate", view: "deck", now: 0 }, { type: "modal", name: "detail", now: 1 })
    assert.equal(Nav.nextDeadline(s), 1 + T)
    s = run(s, { type: "tick", now: 1 + T })
    assert.equal(s.view, "home")
    assert.equal(s.modal, "")
    // On Home, an open history also closes after the timeout.
    s = run(start(), { type: "modal", name: "notifications", now: 5 })
    s = run(s, { type: "tick", now: 5 + T })
    assert.equal(s.modal, "")
    // Decisions still suspend it.
    for (const m of ["confirm", "edit"]) {
        const b = run(start(), { type: "navigate", view: "deck", now: 0 }, { type: "modal", name: m, now: 1 })
        assert.equal(Nav.nextDeadline(b), -1, m)
    }
})
