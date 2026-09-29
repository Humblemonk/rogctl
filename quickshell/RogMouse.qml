pragma Singleton

// Mouse battery status from `rogctl watch --json`, for Quickshell configs.
// Copy this file next to your shell.qml and read `RogMouse.battery` etc. from
// any bar; a singleton runs one rogctl however many bars show it.

import QtQuick
import Quickshell
import Quickshell.Io

Singleton {
    id: root

    // Settings: edit these here.
    property string binary: "rogctl"
    property int interval: 60
    // Notify at or below this level; 0 turns it off.
    property int lowThreshold: 20

    // The last status line: state ("connected", "asleep", "disconnected" or
    // "error"), device, battery, charging, error, ... See README.md.
    property var status: null

    // Not `state`: that name is taken by QML's own states.
    readonly property string mouseState: status?.state ?? ""
    readonly property string device: status?.device ?? ""
    // -1 while unknown.
    readonly property int battery: status?.battery ?? -1
    readonly property bool charging: status?.charging ?? false
    readonly property bool connected: mouseState === "connected"
    readonly property bool low: lowThreshold > 0 && battery >= 0 && !charging && battery <= lowThreshold

    // Solaar's wording, as in the other rogctl widgets.
    readonly property string batteryText: {
        if (battery < 0)
            return mouseState === "disconnected" ? "unknown" : "offline";
        let word = "discharging";
        if (!connected)
            word = "offline";
        else if (charging)
            word = battery === 100 ? "full" : "recharging";
        return `${battery}% (${word})`;
    }

    signal lowBattery

    property bool _lowNotified: false

    // Read the mouse now instead of waiting for the next interval.
    function refresh() {
        oneShot.running = true;
    }

    function _publish(line) {
        let s;
        try {
            s = JSON.parse(line);
        } catch (e) {
            console.warn("RogMouse: bad line from rogctl:", line);
            return;
        }
        // rogctl keeps the level while the mouse sleeps, but a one-shot read
        // starts fresh, so carry it over here too.
        if (s.state === "asleep" && s.battery === undefined && status?.device === s.device) {
            s.battery = status.battery;
            s.battery_updated = status.battery_updated;
        }
        status = s;
        _checkLow();
    }

    function _checkLow() {
        if (lowThreshold <= 0 || !connected || battery < 0)
            return;
        if (charging || battery > lowThreshold + 5) {
            _lowNotified = false;
        } else if (battery <= lowThreshold && !_lowNotified) {
            _lowNotified = true;
            lowBattery();
            Quickshell.execDetached(["notify-send", "--icon=input-mouse", device || "Mouse", `Battery: ${batteryText}`]);
        }
    }

    Process {
        id: watcher
        command: [root.binary, "watch", "--json", "--interval", String(root.interval)]
        running: true
        stdout: SplitParser {
            onRead: line => root._publish(line)
        }
        onExited: (code, exitStatus) => {
            root.status = {
                state: "error",
                charging: false,
                error: `${root.binary} exited with status ${code}`
            };
        }
        onRunningChanged: {
            if (!running)
                restartTimer.start();
        }
    }

    // Start rogctl again after it exits or fails to start, e.g. if it wasn't
    // on PATH yet.
    Timer {
        id: restartTimer
        interval: 10000
        onTriggered: watcher.running = true
    }

    Process {
        id: oneShot
        command: [root.binary, "battery", "--json"]
        stdout: SplitParser {
            onRead: line => root._publish(line)
        }
    }
}
