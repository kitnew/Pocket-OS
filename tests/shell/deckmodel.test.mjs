// Unit tests for the Control Deck model (apps/shell/state/deckmodel.js).
//   node --test tests/shell/        (part of scripts/check.sh)
import { test } from "node:test"
import assert from "node:assert/strict"
import { readFileSync } from "node:fs"

const src = readFileSync(new URL("../../apps/shell/state/deckmodel.js", import.meta.url), "utf8")
    .replace(/^\.pragma library\s*$/m, "")
const D = new Function(src + `
return { tileState, humanReason, humanError, options, defaultIcon, choiceParam, clone, addTile,
         updateTile, removeTile, moveTile, moveTileToPage, addPage, renamePage, removePage,
         movePage, toJson, MAX_TILES }`)()

const host = "nikitapc"
const launch = (available, reason, choices) => ({
    id: "app.launch", label: "Open application", category: "apps", privilege: "elevated",
    available, reason: reason || "",
    params: [{ name: "app", kind: "string", required: true, choices: choices || [] }]
})
const konsole = [{ value: "org.kde.konsole", label: "Konsole" }]
const tile = (action, params) => ({ id: "p/0", action, label: "T", icon: "", params: params || {} })
const ctx = (o) => Object.assign({ connected: true, pending: false, result: null, showResult: false, host }, o)

test("tile kinds follow the documented priority", () => {
    const t = tile("app.launch", { app: "org.kde.konsole" })
    const ok = launch(true, "", konsole)
    assert.equal(D.tileState(t, ok, ctx({ pending: true })).kind, "running")
    assert.equal(D.tileState(t, ok, ctx({ connected: false })).kind, "offline")
    assert.equal(D.tileState(t, null, ctx()).kind, "missing")
    const ng = launch(false, "not granted on this computer (pocket-desktop-agent grant app.launch)", konsole)
    const s = D.tileState(t, ng, ctx())
    assert.equal(s.kind, "unauthorized")
    assert.equal(s.status, "Needs permission")
    assert.ok(s.tappable, "opens the explanation / Ask for access")
    assert.match(s.detail, /grant app.launch/)
    assert.equal(D.tileState(t, ok, ctx()).kind, "ready")
})

test("a tile naming an app the laptop does not offer is 'not set up', not broken", () => {
    const t = tile("app.launch", { app: "org.mozilla.firefox" })
    const s = D.tileState(t, launch(true, "", konsole), ctx())
    assert.equal(s.kind, "unconfigured")
    assert.equal(s.status, "Not set up")
    assert.match(s.detail, /apps add org.mozilla.firefox/)
    // A tile without the required choice parameter is also unconfigured.
    assert.equal(D.tileState(tile("app.launch"), launch(true, "", konsole), ctx()).kind, "unconfigured")
})

test("results: uncertain, failed, done (briefly), then ready", () => {
    const t = tile("session.lock")
    const a = { id: "session.lock", available: true, privilege: "elevated", params: [] }
    assert.equal(D.tileState(t, a, ctx({ result: { ok: false, uncertain: true, code: "timeout" } })).kind, "uncertain")
    const f = D.tileState(t, a, ctx({ result: { ok: false, code: "failed", text: "screen locker: boom" } }))
    assert.equal(f.kind, "failed")
    assert.equal(f.status, "Screen locker: boom")
    assert.equal(D.tileState(t, a, ctx({ result: { ok: true, text: "Done on nikitapc" }, showResult: true })).kind, "done")
    assert.equal(D.tileState(t, a, ctx({ result: { ok: true, text: "x" }, showResult: false })).kind, "ready")
})

test("raw agent reasons read like product copy", () => {
    assert.equal(D.humanReason("no media player is running"), "Nothing playing")
    assert.equal(D.humanReason("KDE global shortcuts (kwin) are not available"), "KDE shortcuts unavailable")
    assert.equal(D.humanReason("no applications registered (pocket-desktop-agent apps add <desktop-id>)"), "No apps set up")
    assert.equal(D.humanReason("something odd (detail)"), "Something odd")
    assert.equal(D.humanReason(""), "")
    assert.equal(D.humanError({ code: "not_permitted", text: "not granted on this computer" }), "Needs permission")
    assert.equal(D.humanError({ code: "invalid_params", text: "x" }), "Tile settings rejected")
})

test("picker options: one per closed-list choice, never access.request", () => {
    const actions = [
        launch(true, "", [...konsole, { value: "code", label: "Visual Studio Code" }]),
        { id: "desktop.notify", label: "Desktop notification", category: "desktop", available: true, params: [
            { name: "title", kind: "string" }, { name: "body", kind: "string" }] },
        { id: "access.request", label: "Ask", available: true, params: [
            { name: "action", kind: "string", required: true, choices: [{ value: "app.launch", label: "x" }] }] },
        { id: "session.lock", label: "Lock screen", category: "session", available: false,
          privilege: "elevated", reason: "not granted", params: [] }
    ]
    const o = D.options(actions)
    assert.deepEqual(o.map(x => x.key), ["app.launch:org.kde.konsole", "app.launch:code", "desktop.notify", "session.lock"])
    assert.deepEqual(o[1].params, { app: "code" })
    assert.equal(o[1].icon, "code")
    assert.equal(o[0].icon, "terminal")
    assert.equal(o[2].params.title, "Pocket OS", "free-text actions get a fixed preset")
    assert.equal(o[3].available, false, "ungranted actions can still be placed")
})

const deck = () => ({ version: 1, navigation: { idle_timeout_s: 15 }, source: "default", error: "",
    pages: [{ id: "general", title: "General", tiles: [tile("a.a"), tile("b.b"), tile("c.c")] },
            { id: "media", title: "Media", tiles: [] }] })

test("editing works on a copy and keeps ids positional", () => {
    const d0 = deck()
    const d1 = D.moveTile(d0, "general", 0, 1)
    assert.deepEqual(d1.pages[0].tiles.map(t => t.action), ["b.b", "a.a", "c.c"])
    assert.deepEqual(d1.pages[0].tiles.map(t => t.id), ["general/0", "general/1", "general/2"])
    assert.deepEqual(d0.pages[0].tiles.map(t => t.action), ["a.a", "b.b", "c.c"], "input untouched")
    assert.deepEqual(D.moveTile(d0, "general", 0, -1).pages[0].tiles.map(t => t.action), ["a.a", "b.b", "c.c"])
    const d2 = D.moveTileToPage(d1, "general", 2, "media")
    assert.equal(d2.pages[1].tiles[0].action, "c.c")
    assert.equal(d2.pages[1].tiles[0].id, "media/0")
    const d3 = D.removeTile(d2, "general", 0)
    assert.equal(d3.pages[0].tiles.length, 1)
    const d4 = D.addTile(d3, "general", { action: "app.launch", params: { app: "code" }, label: "VS Code" }, "  Editor   ")
    assert.deepEqual(d4.pages[0].tiles[1], { action: "app.launch", label: "Editor", icon: "code", params: { app: "code" }, id: "general/1" })
    const d5 = D.updateTile(d4, "general", 1, { label: "", icon: "star" })
    assert.equal(d5.pages[0].tiles[1].label, "Editor", "an empty label keeps the old one")
    assert.equal(d5.pages[0].tiles[1].icon, "star")
})

test("pages: add with a unique id, rename, move, never delete the last one", () => {
    let d = D.addPage(deck(), "Media")
    assert.equal(d.pages[2].id, "media-2")
    d = D.addPage(d, "123")
    assert.match(d.pages[3].id, /^page-123$/)
    d = D.renamePage(d, "general", "  Desk ")
    assert.equal(d.pages[0].title, "Desk")
    d = D.movePage(d, "general", 1)
    assert.equal(d.pages[1].id, "general")
    let one = { version: 1, pages: [{ id: "a", title: "A", tiles: [] }] }
    assert.equal(D.removePage(one, "a").pages.length, 1)
})

test("toJson drops generated ids and metadata", () => {
    const j = JSON.parse(D.toJson(deck()))
    assert.equal(j.source, undefined)
    assert.equal(j.pages[0].tiles[0].id, undefined)
    assert.equal(j.version, 1)
})

test("tile limit is enforced in the editor too", () => {
    let d = deck()
    for (let i = 0; i < 30; ++i)
        d = D.addTile(d, "media", { action: "x.y", params: {}, label: "X" })
    assert.equal(d.pages[1].tiles.length, D.MAX_TILES)
})
