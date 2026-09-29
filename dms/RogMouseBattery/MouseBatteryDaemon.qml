// Runs `rogctl watch --json` and publishes each status line as the "status"
// global var for the bar widgets. Also sends the low-battery notification
// when rogctl flags one (`notify_low`), here rather than in the widget so it
// fires once however many bars show it.

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
    property bool restarting: false

    onBinaryChanged: restart()
    onIntervalChanged: restart()
    onLowThresholdChanged: restart()

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
        publish(status);
        if (status.notify_low)
            Quickshell.execDetached(["notify-send", "-a", "rogctl", "-i", "input-mouse", status.device || tr("Mouse"), tr("Battery: %1% (%2)").arg(status.battery).arg(statusWord(status))]);
    }

    Process {
        id: watcher
        command: [root.binary, "watch", "--json", "--interval", String(root.interval), "--low-threshold", String(root.lowThreshold)]
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

    // The widget asks for a fresh reading by setting "refresh". SIGUSR1 makes
    // the running rogctl read the mouse right away.
    Connections {
        target: PluginService
        function onGlobalVarChanged(changedId, varName) {
            if (changedId === root.pluginId && varName === "refresh" && watcher.running && root.lastStatus?.pid)
                Quickshell.execDetached(["kill", "-USR1", String(root.lastStatus.pid)]);
        }
    }
}
