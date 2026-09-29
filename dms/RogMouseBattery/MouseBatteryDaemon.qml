// Runs `rogctl watch --json` and publishes each status line as the "status"
// global var for the bar widgets. Also sends the low-battery notification,
// here rather than in the widget so it fires once however many bars show it.

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
    readonly property int interval: pluginData.interval || 60
    readonly property int lowThreshold: pluginData.lowThreshold ?? 20

    property var lastStatus: null
    property bool lowNotified: false
    property bool restarting: false

    onBinaryChanged: restart()
    onIntervalChanged: restart()

    function tr(text) {
        return I18n.trFor("rogMouseBattery", text);
    }

    // Solaar's status words: discharging, recharging, full.
    function statusWord(s) {
        if (s.charging && s.battery === 100)
            return tr("full");
        return s.charging ? tr("recharging") : tr("discharging");
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

    function handleLine(line) {
        let status;
        try {
            status = JSON.parse(line);
        } catch (e) {
            console.warn("rogMouseBattery: bad line from rogctl:", line);
            return;
        }
        // rogctl keeps the level while the mouse sleeps, but a one-shot read
        // (the widget's refresh) starts fresh, so carry it over here too.
        if (status.state === "asleep" && status.battery === undefined && lastStatus?.device === status.device) {
            status.battery = lastStatus.battery;
            status.low_battery_warning = lastStatus.low_battery_warning;
            status.power_off_minutes = lastStatus.power_off_minutes;
            status.battery_updated = lastStatus.battery_updated;
        }
        publish(status);
        checkLowBattery(status);
    }

    function checkLowBattery(s) {
        if (lowThreshold <= 0 || s.battery === undefined || s.state !== "connected")
            return;
        if (s.charging || s.battery > lowThreshold + 5) {
            lowNotified = false;
        } else if (s.battery <= lowThreshold && !lowNotified) {
            lowNotified = true;
            Quickshell.execDetached(["notify-send", "-a", "rogctl", "-i", "input-mouse", s.device || tr("Mouse"), tr("Battery: %1% (%2)").arg(s.battery).arg(statusWord(s))]);
        }
    }

    Process {
        id: watcher
        command: [root.binary, "watch", "--json", "--interval", String(root.interval)]
        running: true
        stdout: SplitParser {
            onRead: line => root.handleLine(line)
        }
        onExited: (exitCode, exitStatus) => {
            if (root.restarting)
                return;
            root.publish({
                state: "error",
                charging: false,
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
            watcher.running = true;
        }
    }

    Process {
        id: oneShot
        command: [root.binary, "battery", "--json"]
        stdout: SplitParser {
            onRead: line => root.handleLine(line)
        }
    }

    // The widget asks for a fresh reading by setting "refresh".
    Connections {
        target: PluginService
        function onGlobalVarChanged(changedId, varName) {
            if (changedId === root.pluginId && varName === "refresh" && !oneShot.running)
                oneShot.running = true;
        }
    }
}
