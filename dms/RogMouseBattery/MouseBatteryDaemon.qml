// Runs `rogctl watch --json --device ...` and publishes each line as the
// "status" global var for the bar widgets, always as `{ devices, pid }` (or
// `error`): the statuses to show, an empty list while nothing is connected.
// Also sends each low-battery notification when rogctl flags one
// (`notify_low`), here rather than in the widget so it fires once however
// many bars show it.

import QtQuick
import Quickshell
import Quickshell.Io
import qs.Common
import qs.Services
import qs.Modules.Plugins

PluginComponent {
    id: root

    property var popoutService: null

    readonly property string binary: pluginData.binary || "rogctl"
    // Passed to `--device`: all, mouse or keyboard.
    readonly property string device: ["mouse", "keyboard"].includes(pluginData.device) ? pluginData.device : "all"
    readonly property int interval: pluginData.interval || 60
    readonly property int lowThreshold: pluginData.lowThreshold ?? 20

    property var lastStatus: null
    property bool restarting: false

    onBinaryChanged: restart()
    onDeviceChanged: restart()
    onIntervalChanged: restart()
    onLowThresholdChanged: restart()

    function tr(text) {
        return I18n.trFor("rogMouseBattery", text);
    }

    // Status words as the kernel and desktops use them: discharging, charging,
    // full. Solaar says "recharging"; rogctl doesn't.
    function statusWord(s) {
        if (s.charging && s.battery === 100)
            return tr("full");
        return s.charging ? tr("charging") : tr("discharging");
    }

    function restart() {
        if (!watcher.running)
            return;
        restarting = true;
        restartTimer.interval = 500;
        watcher.running = false;
    }

    function publish(status) {
        lastStatus = status;
        PluginService.setGlobalVar(pluginId, "status", status);
    }

    // `--device all` prints `{ devices, pid }`; mouse or keyboard one status,
    // which is left out of the list while disconnected.
    function report(line) {
        if (line.devices !== undefined)
            return line;
        return {
            devices: line.state === "disconnected" ? [] : [line],
            pid: line.pid
        };
    }

    function handleLine(line) {
        let status;
        try {
            status = report(JSON.parse(line));
        } catch (e) {
            console.warn("rogMouseBattery: bad line from rogctl:", line);
            return;
        }
        publish(status);
        for (const s of status.devices) {
            if (!s.notify_low)
                continue;
            const keyboard = s.kind === "keyboard";
            Quickshell.execDetached(["notify-send", "-a", "rogctl", "-i", keyboard ? "input-keyboard" : "input-mouse", s.device || tr(keyboard ? "Keyboard" : "Mouse"), tr("Battery: %1% (%2)").arg(s.battery).arg(statusWord(s))]);
        }
    }

    Process {
        id: watcher
        command: [root.binary, "watch", "--json", "--device", root.device, "--interval", String(root.interval), "--low-threshold", String(root.lowThreshold)]
        running: true
        stdout: SplitParser {
            onRead: line => root.handleLine(line)
        }
        onExited: (exitCode, exitStatus) => {
            if (root.restarting)
                return;
            root.publish({
                devices: [],
                error: root.tr("%1 exited with status %2").arg(root.binary).arg(exitCode)
            });
        }
        onRunningChanged: {
            if (!running)
                restartTimer.start();
        }
    }

    // Start rogctl again after it exits or fails to start, e.g. if it wasn't
    // on PATH yet, or right away after a settings change.
    Timer {
        id: restartTimer
        interval: 10000
        onTriggered: {
            interval = 10000;
            root.restarting = false;
            // Forget the last rogctl's pid: until the new one prints a line,
            // there's nothing to signal.
            root.lastStatus = null;
            watcher.running = true;
        }
    }

    // The widget asks for a fresh reading by setting "refresh". `rogctl
    // refresh` makes the running rogctl read the devices right away; it signals
    // the pid only if it's a `rogctl watch`, unlike `kill`.
    Connections {
        target: PluginService
        function onGlobalVarChanged(changedId, varName) {
            if (changedId === root.pluginId && varName === "refresh" && watcher.running && !root.restarting && root.lastStatus?.pid)
                Quickshell.execDetached([root.binary, "refresh", String(root.lastStatus.pid)]);
        }
    }
}
