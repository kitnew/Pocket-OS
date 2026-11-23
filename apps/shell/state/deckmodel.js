.pragma library
// Control Deck model (docs/dock-mode/ux-specification.md, "Tile states" and "Editing").
//
// Pure functions, no QML: what a tile shows given the laptop's advertisement and the last
// result, how raw agent reasons read for a person, and the deck editing operations. Used
// by the QML deck and editor; tested under node (tests/shell/deckmodel.test.mjs).
//
// Tile kinds (one per tile, in priority order):
//   running       Core accepted the request; waiting for the laptop
//   offline       no link to the laptop
//   missing       the laptop does not offer the tile's action
//   unauthorized  elevated action the laptop owner has not allowed
//   unconfigured  the tile names an app/place/shortcut the laptop does not offer
//   unavailable   offered but cannot run now (reason from the laptop)
//   uncertain     the last request may or may not have run (tap = Retry)
//   failed        the last request failed (stays until the next tap)
//   done          the last request succeeded (shown briefly)
//   ready         tap to run

var ACCESS_ID = "access.request"

// The parameter of an action that selects from the laptop's closed list, or null.
function choiceParam(action) {
    if (!action || !action.params)
        return null
    for (var i = 0; i < action.params.length; ++i) {
        var p = action.params[i]
        if (p.choices && p.choices.length >= 0 && (p.choices.length > 0 || p.required))
            return p
    }
    return null
}

function choiceLabel(param, value) {
    if (!param || !param.choices)
        return ""
    for (var i = 0; i < param.choices.length; ++i)
        if (param.choices[i].value === value)
            return param.choices[i].label
    return ""
}

// A short, human sentence for a raw agent reason ("" when there is none).
function humanReason(reason) {
    var r = String(reason || "").trim()
    if (r === "")
        return ""
    var rules = [
        [/^not granted/i, "Needs permission"],
        [/no applications registered/i, "No apps set up"],
        [/none of the registered applications/i, "App not installed"],
        [/no destinations configured/i, "No places set up"],
        [/reading the installed applications/i, "Loading apps…"],
        [/KDE global shortcuts .*not available/i, "KDE shortcuts unavailable"],
        [/KDE has no/i, "Not supported by KDE"],
        [/no desktop session bus/i, "No desktop session"],
        [/no media player/i, "Nothing playing"],
        [/already allowed/i, "Everything allowed"],
        [/is not installed/i, "App not installed"],
        [/does not exist/i, "Folder not found"],
        [/disabled in the agent configuration/i, "Disabled on the laptop"],
        [/already waiting for an answer/i, "Waiting for an answer"],
        [/another request is waiting/i, "Another request is open"],
        [/denied on the computer/i, "Denied on the laptop"]
    ]
    for (var i = 0; i < rules.length; ++i)
        if (rules[i][0].test(r))
            return rules[i][1]
    r = r.replace(/\s*\([^)]*\)\s*$/, "")
    return r.charAt(0).toUpperCase() + r.slice(1)
}

// A short text for a failed result ({code, text}).
function humanError(result) {
    if (!result)
        return ""
    switch (result.code) {
    case "not_permitted": return /not granted/i.test(result.text || "") ? "Needs permission" : "Not allowed on the laptop"
    case "unknown_action": return "Not offered by the laptop"
    case "invalid_params": return "Tile settings rejected"
    case "core_lost": return "Pocket Core restarted"
    case "NotConnected": return "Not connected"
    case "unavailable": return humanReason(result.text) || "Unavailable"
    default: return humanReason(result.text) || "Failed"
    }
}

// What a tile looks like now. ctx: { connected, pending, result, showResult, host }
// Returns { kind, status, detail, tappable }.
function tileState(tile, action, ctx) {
    var host = ctx.host || "the laptop"
    var result = ctx.result || null
    if (ctx.pending)
        return { kind: "running", status: result && result.progress ? result.text : "Running…",
                 detail: "Waiting for " + host + " to confirm.", tappable: false }
    if (!ctx.connected)
        return { kind: "offline", status: "Offline", detail: host + " is not connected.", tappable: false }
    if (!action)
        return { kind: "missing", status: "Not on " + host,
                 detail: host + " does not offer “" + (tile ? tile.action : "") + "”. Its agent may be older, or the action is disabled there.",
                 tappable: false }
    if (!action.available && action.privilege === "elevated" && /^not granted/i.test(action.reason || ""))
        return { kind: "unauthorized", status: "Needs permission",
                 detail: "“" + (action.label || action.id) + "” is not allowed on " + host + " yet. Ask for access and approve it on the laptop, or run “pocket-desktop-agent grant " + action.id + "” there.",
                 tappable: true }
    var p = choiceParam(action)
    if (p && tile) {
        var v = tile.params ? tile.params[p.name] : undefined
        if (v === undefined || choiceLabel(p, v) === "") {
            var what = p.name === "app" ? "app" : p.name === "destination" ? "place" : p.name
            return { kind: "unconfigured", status: "Not set up",
                     detail: (v === undefined ? "This tile does not choose a " + what + "."
                                              : "“" + v + "” is not a " + what + " " + host + " offers.")
                             + (p.name === "app" ? " On the laptop: pocket-desktop-agent apps add " + (v || "<desktop-id>") + "."
                                : p.name === "destination" ? " On the laptop: pocket-desktop-agent destinations add …" : ""),
                     tappable: true }
        }
    }
    if (!action.available)
        return { kind: "unavailable", status: humanReason(action.reason) || "Unavailable",
                 detail: action.reason || "", tappable: false }
    if (result && result.uncertain)
        return { kind: "uncertain", status: "Tap to retry",
                 detail: "The laptop may or may not have run it. A retry never runs it twice.", tappable: true }
    if (result && !result.ok && !result.progress)
        return { kind: "failed", status: humanError(result), detail: result.text || "", tappable: true }
    if (result && result.ok && !result.progress && ctx.showResult)
        return { kind: "done", status: result.text && result.text.length <= 24 ? result.text : "Done",
                 detail: result.text || "", tappable: true }
    return { kind: "ready", status: "", detail: "", tappable: true }
}

// ── Picker ───────────────────────────────────────────────────────────────────────
var ICONS = {
    "session.lock": "lock", "desktop.notify": "bell", "media.play_pause": "play-pause",
    "media.next": "skip-forward", "media.previous": "skip-back", "volume.up": "volume",
    "volume.down": "volume-down", "volume.mute": "volume-off", "workflow.run": "workflow",
    "desktop.open": "folder", "app.launch": "apps"
}
var SHORTCUT_ICONS = { overview: "windows", show_desktop: "desktop", desktop_next: "chevron-right",
                       desktop_previous: "chevron-left", grid_view: "grid" }
var APP_ICONS = [[/konsole|terminal|console|foot|alacritty|kitty/i, "terminal"],
                 [/dolphin|nautilus|files|thunar/i, "folder"],
                 [/firefox|chrom|browser|brave|epiphany/i, "globe"],
                 [/code|kate|zed|idea|jetbrains|editor|vim/i, "code"],
                 [/music|spotify|elisa|rhythmbox/i, "music"],
                 [/settings/i, "sliders"]]

function defaultIcon(actionId, params) {
    params = params || {}
    if (actionId === "desktop.shortcut" && SHORTCUT_ICONS[params.shortcut])
        return SHORTCUT_ICONS[params.shortcut]
    if (actionId === "app.launch" && params.app)
        for (var i = 0; i < APP_ICONS.length; ++i)
            if (APP_ICONS[i][0].test(params.app))
                return APP_ICONS[i][1]
    if (actionId === "desktop.open" && params.destination && /https?:/.test(params.destination))
        return "globe"
    return ICONS[actionId] || "action"
}

// Fixed parameters for actions that take free text (the phone never asks for any).
var PRESETS = { "desktop.notify": { title: "Pocket OS", body: "Ping from your phone" } }

// Everything a tile can be: one option per advertised action, or per choice of its
// closed list. Unavailable/unauthorized actions are included (marked), so a tile can be
// placed before access is granted. Returns [{ key, action, params, label, group, icon, available, privilege }].
function options(actions) {
    var out = []
    for (var i = 0; i < (actions || []).length; ++i) {
        var a = actions[i]
        if (a.id === ACCESS_ID)
            continue
        var p = choiceParam(a)
        var base = { action: a.id, group: a.category || "other", available: a.available,
                     privilege: a.privilege || "basic" }
        if (p) {
            for (var j = 0; j < p.choices.length; ++j) {
                var c = p.choices[j], params = {}
                params[p.name] = c.value
                out.push(Object.assign({}, base, { key: a.id + ":" + c.value, params: params, label: c.label,
                                                   icon: defaultIcon(a.id, params) }))
            }
        } else {
            out.push(Object.assign({}, base, { key: a.id, params: Object.assign({}, PRESETS[a.id] || {}),
                                               label: a.label || a.id, icon: defaultIcon(a.id, {}) }))
        }
    }
    return out
}

// ── Editing (operations on a deep copy; Core validates the result on save) ──────────
var MAX_PAGES = 12
var MAX_TILES = 24
var MAX_LABEL = 40
var MAX_TITLE = 32

function clone(deck) {
    var d = JSON.parse(JSON.stringify(deck || {}))
    return { version: 1, navigation: d.navigation || { idle_timeout_s: 15 }, pages: d.pages || [] }
}
function pageIndex(deck, pageId) {
    for (var i = 0; i < deck.pages.length; ++i)
        if (deck.pages[i].id === pageId)
            return i
    return -1
}
function clampText(s, max) {
    s = String(s || "").replace(/\s+/g, " ").trim()
    return s.length > max ? s.slice(0, max) : s
}
// Renumbers tile ids ("page/index") so the working copy matches what Core will serve.
function renumber(deck) {
    for (var i = 0; i < deck.pages.length; ++i)
        for (var j = 0; j < deck.pages[i].tiles.length; ++j)
            deck.pages[i].tiles[j].id = deck.pages[i].id + "/" + j
    return deck
}
function addTile(deck, pageId, option, label) {
    var d = clone(deck), i = pageIndex(d, pageId)
    if (i < 0 || d.pages[i].tiles.length >= MAX_TILES)
        return d
    d.pages[i].tiles.push({ action: option.action, label: clampText(label || option.label, MAX_LABEL) || "Tile",
                            icon: option.icon || defaultIcon(option.action, option.params),
                            params: Object.assign({}, option.params || {}) })
    return renumber(d)
}
function updateTile(deck, pageId, index, changes) {
    var d = clone(deck), i = pageIndex(d, pageId)
    if (i < 0 || index < 0 || index >= d.pages[i].tiles.length)
        return d
    var t = d.pages[i].tiles[index]
    if (changes.label !== undefined)
        t.label = clampText(changes.label, MAX_LABEL) || t.label
    if (changes.icon !== undefined)
        t.icon = changes.icon
    if (changes.option) {
        t.action = changes.option.action
        t.params = Object.assign({}, changes.option.params || {})
    }
    return renumber(d)
}
function removeTile(deck, pageId, index) {
    var d = clone(deck), i = pageIndex(d, pageId)
    if (i >= 0 && index >= 0 && index < d.pages[i].tiles.length)
        d.pages[i].tiles.splice(index, 1)
    return renumber(d)
}
function moveTile(deck, pageId, index, delta) {
    var d = clone(deck), i = pageIndex(d, pageId)
    if (i < 0)
        return d
    var tiles = d.pages[i].tiles, to = index + delta
    if (index < 0 || index >= tiles.length || to < 0 || to >= tiles.length)
        return d
    var t = tiles.splice(index, 1)[0]
    tiles.splice(to, 0, t)
    return renumber(d)
}
function moveTileToPage(deck, pageId, index, toPageId) {
    var d = clone(deck), i = pageIndex(d, pageId), k = pageIndex(d, toPageId)
    if (i < 0 || k < 0 || i === k || index < 0 || index >= d.pages[i].tiles.length
            || d.pages[k].tiles.length >= MAX_TILES)
        return d
    d.pages[k].tiles.push(d.pages[i].tiles.splice(index, 1)[0])
    return renumber(d)
}
function slug(title, taken) {
    var base = String(title || "").toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-+|-+$/g, "")
    if (!/^[a-z]/.test(base))
        base = "page" + (base ? "-" + base : "")
    base = base.slice(0, 28)
    var id = base, n = 2
    while (taken.indexOf(id) >= 0)
        id = base + "-" + n++
    return id
}
function addPage(deck, title) {
    var d = clone(deck)
    if (d.pages.length >= MAX_PAGES)
        return d
    var t = clampText(title, MAX_TITLE) || "New page"
    d.pages.push({ id: slug(t, d.pages.map(function (p) { return p.id })), title: t, tiles: [] })
    return d
}
function renamePage(deck, pageId, title) {
    var d = clone(deck), i = pageIndex(d, pageId), t = clampText(title, MAX_TITLE)
    if (i >= 0 && t)
        d.pages[i].title = t
    return d
}
function removePage(deck, pageId) {
    var d = clone(deck), i = pageIndex(d, pageId)
    if (i >= 0 && d.pages.length > 1)
        d.pages.splice(i, 1)
    return d
}
function movePage(deck, pageId, delta) {
    var d = clone(deck), i = pageIndex(d, pageId), to = i + delta
    if (i < 0 || to < 0 || to >= d.pages.length)
        return d
    var p = d.pages.splice(i, 1)[0]
    d.pages.splice(to, 0, p)
    return d
}
// The document sent to Core's SetDeck.
function toJson(deck) {
    var d = clone(deck)
    for (var i = 0; i < d.pages.length; ++i)
        for (var j = 0; j < d.pages[i].tiles.length; ++j)
            delete d.pages[i].tiles[j].id
    return JSON.stringify(d)
}
