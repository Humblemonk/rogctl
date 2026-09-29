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
    // At or below lowThreshold and not charging; rogctl decides.
    readonly property bool low: status?.low ?? false

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

    // Emitted once per drop to the threshold, when rogctl flags it.
    signal lowBattery

    // Read the mouse now instead of waiting for the next interval: SIGUSR1
    // makes the running rogctl read it right away.
    function refresh() {
        if (watcher.running && status?.pid)
            Quickshell.execDetached(["kill", "-USR1", String(status.pid)]);
    }

    function _publish(line) {
        let s;
        try {
            s = JSON.parse(line);
        } catch (e) {
            console.warn("RogMouse: bad line from rogctl:", line);
            return;
        }
        status = s;
        if (s.notify_low) {
            lowBattery();
            Quickshell.execDetached(["notify-send", "--icon=input-mouse", device || "Mouse", `Battery: ${batteryText}`]);
        }
    }

    Process {
        id: watcher
        command: [root.binary, "watch", "--json", "--interval", String(root.interval), "--low-threshold", String(root.lowThreshold)]
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
}
