import QtQuick
import Quickshell
import Quickshell.Services.Pipewire

// Audio adapter (Quickshell-specific): volume and mute of the default output, through
// Quickshell's native PipeWire client (no wpctl, no polling). PipeWire and WirePlumber
// own the state (devices, routing, default sink, persisted volumes); nothing is mirrored
// into Pocket Core. Every change, including external ones (wpctl, apps, jack switching),
// arrives as a PipeWire event and updates these properties.
//
// When PipeWire is not running, `connected` is false; Quickshell reconnects by itself
// when a new pipewire-0 socket appears (audio component restart).
Scope {
    id: root

    readonly property var _sink: Pipewire.defaultAudioSink
    readonly property var _audio: _sink && _sink.ready ? _sink.audio : null

    // Volume and mute are valid only for bound nodes.
    PwObjectTracker { objects: root._sink ? [root._sink] : [] }

    readonly property bool connected: Pipewire.ready
    readonly property bool available: connected && _audio !== null
    readonly property real volume: _audio ? Math.min(1, _audio.volume) : 0
    readonly property bool muted: _audio !== null && _audio.muted
    readonly property string outputName: _sink ? (_sink.description || _sink.nickname || _sink.name) : ""
    readonly property int outputId: _sink ? _sink.id : -1

    // 0..1 (no software amplification above 100 %). Raising the volume unmutes.
    function setVolume(v) {
        if (!_audio)
            return false
        _audio.volume = Math.max(0, Math.min(1, v))
        if (_audio.muted && v > 0)
            _audio.muted = false
        return true
    }
    function setMuted(m) {
        if (!_audio)
            return false
        _audio.muted = m
        return true
    }
}
