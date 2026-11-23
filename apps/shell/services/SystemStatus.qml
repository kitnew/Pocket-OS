import QtQuick
import Quickshell
import Quickshell.Services.UPower
import Quickshell.Networking
import Quickshell.Bluetooth

// System status adapter (Quickshell-specific): battery (UPower), network
// (NetworkManager) and Bluetooth (BlueZ) through Quickshell's service integrations.
// The portable UI reads only these plain properties. Each `…Available` is false when
// the service or hardware is missing, and the UI then shows "unavailable" instead of a
// guess. Nothing here is mirrored into Pocket Core (system-owned state).
Scope {
    // Battery
    readonly property var _bat: UPower.displayDevice
    readonly property bool batteryAvailable: _bat && _bat.ready && _bat.isPresent
    readonly property real batteryPercent: _bat ? _bat.percentage * 100 : 0
    readonly property bool batteryCharging: _bat !== null && (_bat.state === UPowerDeviceState.Charging
        || _bat.state === UPowerDeviceState.PendingCharge)
    readonly property bool batteryFull: _bat !== null && _bat.state === UPowerDeviceState.FullyCharged
    readonly property bool onBattery: UPower.onBattery

    // Network
    readonly property var _devices: Networking.devices.values
    readonly property var _wifi: {
        for (const d of _devices)
            if (d.type === DeviceType.Wifi)
                return d
        return null
    }
    readonly property bool wifiAvailable: _wifi !== null
    readonly property bool wifiEnabled: Networking.wifiEnabled
    readonly property bool wifiHardwareEnabled: Networking.wifiHardwareEnabled
    readonly property bool wifiConnected: _wifi !== null && _wifi.connected
    readonly property var _wifiNetwork: {
        if (!_wifi)
            return null
        for (const n of _wifi.networks.values)
            if (n.connected)
                return n
        return null
    }
    readonly property string wifiName: _wifiNetwork ? _wifiNetwork.name : ""
    readonly property real wifiSignal: _wifiNetwork ? _wifiNetwork.signalStrength : 0
    readonly property bool wiredConnected: {
        for (const d of _devices)
            if (d.type === DeviceType.Wired && d.connected)
                return true
        return false
    }
    readonly property string connectivity: NetworkConnectivity.toString(Networking.connectivity)
    function setWifiEnabled(on) { Networking.wifiEnabled = on }

    // Bluetooth
    readonly property var _adapter: Bluetooth.defaultAdapter
    readonly property bool bluetoothAvailable: _adapter !== null
    readonly property bool bluetoothEnabled: _adapter !== null && _adapter.enabled
    readonly property bool bluetoothBusy: _adapter !== null
        && (_adapter.state === BluetoothAdapterState.Enabling || _adapter.state === BluetoothAdapterState.Disabling)
    readonly property int bluetoothConnected: {
        if (!_adapter)
            return 0
        let n = 0
        for (const d of _adapter.devices.values)
            if (d.connected)
                ++n
        return n
    }
    function setBluetoothEnabled(on) { if (_adapter) _adapter.enabled = on }
}
