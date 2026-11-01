pragma Singleton

// Mouse and keyboard battery status from `rogctl watch --json`, for
// Quickshell configs. Copy this file and RogMouseSettings.qml next to your
// shell.qml and read `RogMouse.devices` (or `RogMouse.battery` etc. for the
// first device) from any bar; a singleton runs one rogctl however many bars
// show it.

import QtQuick
import Quickshell
import Quickshell.Io

Singleton {
    id: root

    // Settings: edit them in RogMouseSettings.qml; updates keep your changes.
    readonly property string binary: settings.binary
    // Passed to `--device`: all, mouse or keyboard. A settings file from
    // before 2.7.0 has none, which means all.
    readonly property string selection: ["mouse", "keyboard"].includes(settings.device) ? settings.device : "all"
    readonly property int interval: settings.interval
    readonly property int lowThreshold: settings.lowThreshold

    // The last line as `{ devices, pid }` (or `error`): one status per device
    // to show, each with state ("connected", "asleep" or "error"), device,
    // kind ("mouse" or "keyboard"), battery, charging, low, error, ... See
    // docs/cli.md. Empty while nothing is connected.
    property var report: null
    readonly property var devices: report?.devices ?? []
    // Set when rogctl couldn't run.
    readonly property string error: report?.error ?? ""

    // The first device's status, as before 2.7.0; `disconnected` while there
    // is none, `error` while rogctl can't run.
    readonly property var status: {
        if (report === null)
            return null;
        if (error !== "")
            return {
                state: "error",
                charging: false,
                error: error
            };
        return devices.length > 0 ? devices[0] : {
            state: "disconnected",
            charging: false
        };
    }

    // Not `state`: that name is taken by QML's own states.
    readonly property string mouseState: status?.state ?? ""
    readonly property string device: status?.device ?? ""
    // -1 while unknown.
    readonly property int battery: status?.battery ?? -1
    readonly property bool charging: status?.charging ?? false
    readonly property bool connected: mouseState === "connected"
    // At or below lowThreshold and not charging; rogctl decides.
    readonly property bool low: status?.low ?? false

    // The shared wording, as in the other rogctl widgets.
    readonly property string batteryText: status === null ? "" : textFor(status)

    // "79% (discharging)", for any device's status.
    function textFor(s) {
        if (s.battery === undefined)
            return s.state === "disconnected" ? "unknown" : "offline";
        let word = "discharging";
        if (s.state !== "connected")
            word = "offline";
        else if (s.charging)
            word = s.battery === 100 ? "full" : "charging";
        return `${s.battery}% (${word})`;
    }

    // Emitted once per drop to the threshold, when rogctl flags it, with the
    // device's status.
    signal lowBattery(var device)

    // The binary the running rogctl started with: refresh goes through the
    // same one, so a wrapper's pid means the same there.
    property string _watchBinary: ""

    // Read the devices now instead of waiting for the next interval. `rogctl
    // refresh` signals the pid only if it's a `rogctl watch`, unlike `kill`.
    function refresh() {
        if (watcher.running && report?.pid)
            Quickshell.execDetached([root._watchBinary, "refresh", String(report.pid)]);
    }

    // `--device all` prints `{ devices, pid }`; mouse or keyboard one status,
    // which is left out of the list while disconnected.
    function _publish(line) {
        let r;
        try {
            r = JSON.parse(line);
        } catch (e) {
            console.warn("RogMouse: bad line from rogctl:", line);
            return;
        }
        if (r.devices === undefined)
            r = {
                devices: r.state === "disconnected" ? [] : [r],
                pid: r.pid
            };
        report = r;
        for (const s of r.devices) {
            if (!s.notify_low)
                continue;
            lowBattery(s);
            const keyboard = s.kind === "keyboard";
            Quickshell.execDetached(["notify-send", keyboard ? "--icon=input-keyboard" : "--icon=input-mouse", s.device || (keyboard ? "Keyboard" : "Mouse"), `Battery: ${textFor(s)}`]);
        }
    }

    RogMouseSettings {
        id: settings
    }

    Process {
        id: watcher
        command: [root.binary, "watch", "--json", "--device", root.selection, "--interval", String(root.interval), "--low-threshold", String(root.lowThreshold)]
        running: true
        stdout: SplitParser {
            onRead: line => root._publish(line)
        }
        onExited: (code, exitStatus) => {
            root.report = {
                devices: [],
                error: `${root.binary} exited with status ${code}`
            };
        }
        onRunningChanged: {
            if (running)
                root._watchBinary = command[0];
            else
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
