// Panel widget: reads `rogctl battery --json --device ...` every refresh
// interval. The executable data engine only returns output once a command
// exits, so this polls instead of streaming `rogctl watch`. That's also why it
// keeps, for each device, the two bits of state `rogctl watch` keeps for the
// other widgets: the last level while it sleeps, and whether the low-battery
// notification was sent.

import QtQuick
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import org.kde.notification
import org.kde.plasma.components as PlasmaComponents
import org.kde.plasma.core as PlasmaCore
import org.kde.plasma.extras as PlasmaExtras
import org.kde.plasma.plasma5support as P5Support
import org.kde.plasma.plasmoid

PlasmoidItem {
    id: root

    readonly property string binary: Plasmoid.configuration.binary || "rogctl"
    // Passed to `--device`: all, mouse or keyboard.
    readonly property string device: ["mouse", "keyboard"].includes(Plasmoid.configuration.device) ? Plasmoid.configuration.device : "all"
    readonly property int interval: Plasmoid.configuration.interval
    readonly property int lowThreshold: Plasmoid.configuration.lowThreshold

    // The last read as `{ devices }` (or `error`): one status per device to
    // show (state, device, kind, battery, charging, low, error, ...), empty
    // while nothing is connected.
    property var report: null
    readonly property var devices: report?.devices ?? []
    readonly property string errorText: report?.error ?? ""
    // Per device (kind and name, as `rogctl watch` keys them): the last status
    // read while connected, whose level shows once it sleeps even after an
    // error in between, and whether the low-battery notification was sent.
    property var lastConnected: ({})
    property var lowNotified: ({})

    readonly property bool failed: errorText !== "" || devices.some(s => s.state === "error")
    // At or below the threshold and not charging; rogctl decides.
    readonly property bool low: devices.some(s => s.low)
    readonly property bool hidden: Plasmoid.configuration.hideWhenDisconnected && report !== null && errorText === "" && devices.length === 0

    // The shared wording, as in the other rogctl widgets.
    function statusWord(s) {
        if (s.charging && s.battery === 100)
            return i18n("full");
        return s.charging ? i18n("charging") : i18n("discharging");
    }

    function batteryValue(s) {
        if (s.battery === undefined)
            return s.state === "disconnected" ? i18n("unknown") : i18n("offline");
        return i18n("%1% (%2)", s.battery, s.state === "connected" ? statusWord(s) : i18n("offline"));
    }

    function deviceName(s) {
        return s.device || (s.kind === "keyboard" ? i18n("Keyboard") : i18n("Mouse"));
    }

    function detail(s) {
        if (s.state === "error")
            return i18n("Error: %1", s.error || "");
        return i18n("Battery: %1", batteryValue(s));
    }

    // One device: its battery line. Several: a name and line for each.
    readonly property string detailText: {
        if (!report)
            return i18n("Waiting for rogctl…");
        if (errorText !== "")
            return i18n("Error: %1", errorText);
        if (devices.length === 0)
            return i18n("Battery: %1", i18n("unknown"));
        if (devices.length === 1)
            return detail(devices[0]);
        return devices.map(s => deviceName(s) + "\n" + detail(s)).join("\n\n");
    }

    readonly property string headingText: {
        if (devices.length === 1)
            return deviceName(devices[0]);
        return devices.length === 0 ? i18n("Not connected") : i18n("ASUS devices");
    }

    function iconFor(s) {
        if (s.state === "error")
            return "dialog-warning";
        if (s.charging && s.battery !== undefined)
            return "battery-" + String(Math.round(s.battery / 10) * 10).padStart(3, "0") + "-charging";
        return s.kind === "keyboard" ? "input-keyboard" : "input-mouse";
    }

    Plasmoid.icon: devices.length > 0 ? iconFor(devices[0]) : "dialog-warning"
    Plasmoid.status: {
        if (hidden)
            return PlasmaCore.Types.HiddenStatus;
        if (low || failed)
            return PlasmaCore.Types.NeedsAttentionStatus;
        return PlasmaCore.Types.ActiveStatus;
    }

    toolTipMainText: devices.length === 1 ? deviceName(devices[0]) : i18n("ASUS Peripheral Battery")
    toolTipSubText: detailText

    function shellQuote(value) {
        return "'" + String(value).replace(/'/g, "'\\''") + "'";
    }

    function refresh() {
        executable.connectSource(shellQuote(binary) + " battery --json --device " + device + " --low-threshold " + lowThreshold);
    }

    function handleOutput(exitCode, stdout) {
        let r;
        try {
            r = JSON.parse(stdout.trim());
        } catch (e) {
            // No JSON: rogctl is missing or crashed. 127 is the shell's "not found".
            r = {
                devices: [],
                error: exitCode === 127 ? i18n("%1 not found on PATH", binary) : i18n("%1 exited with status %2", binary, exitCode)
            };
        }
        // `--device mouse` or `keyboard` print one status, left out while
        // disconnected.
        if (r.devices === undefined)
            r = {
                devices: r.state === "disconnected" ? [] : [r]
            };
        for (const s of r.devices) {
            const key = (s.kind || "") + "\n" + (s.device || "");
            // A one-shot read forgets the level once the device sleeps; keep
            // showing the last one, as `rogctl watch` does.
            const last = lastConnected[key];
            if (s.state === "connected") {
                lastConnected[key] = s;
            } else if (s.state === "asleep" && s.battery === undefined && last) {
                s.battery = last.battery;
                s.battery_updated = last.battery_updated;
                s.low = last.low;
            }
            checkLowBattery(key, s);
        }
        report = r;
        // Retry an error sooner, since it's often brief. Read a sleeping device
        // every 10 s, as `rogctl watch` does, so one that wakes shows up within
        // 10 s.
        const urgent = r.error !== undefined || r.devices.some(s => s.state === "asleep" || s.state === "error");
        pollTimer.interval = Math.min(urgent ? 10 : interval, interval) * 1000;
        pollTimer.restart();
    }

    // Once per drop to the threshold, re-armed by charging or climbing 5%
    // above it, as `rogctl watch` decides for the other widgets.
    function checkLowBattery(key, s) {
        if (lowThreshold <= 0 || s.battery === undefined || s.state !== "connected")
            return;
        if (s.charging || s.battery > lowThreshold + 5) {
            lowNotified[key] = false;
        } else if (s.low && !lowNotified[key]) {
            lowNotified[key] = true;
            // A notification each: a mouse and a keyboard can go low on the
            // same read, and reusing one object would replace the first.
            // `?.`: a throw here would stop the panel updating.
            lowNotification.createObject(root, {
                title: deviceName(s),
                text: i18n("Battery: %1% (%2)", s.battery, statusWord(s)),
                iconName: s.kind === "keyboard" ? "input-keyboard" : "input-mouse"
            })?.sendEvent();
        }
    }

    onIntervalChanged: {
        pollTimer.interval = interval * 1000;
        pollTimer.restart();
    }
    onBinaryChanged: refresh()
    onDeviceChanged: refresh()
    onLowThresholdChanged: refresh()
    Component.onCompleted: refresh()

    // Set in handleOutput and onIntervalChanged, not bound: errors shorten it.
    Timer {
        id: pollTimer
        interval: 60000
        repeat: true
        running: true
        onTriggered: root.refresh()
    }

    P5Support.DataSource {
        id: executable
        engine: "executable"
        connectedSources: []
        onNewData: (source, data) => {
            disconnectSource(source);
            root.handleOutput(data["exit code"], data["stdout"] || "");
        }
    }

    // Deleted once closed (QML notifications aren't by default).
    Component {
        id: lowNotification
        Notification {
            componentName: "plasma_workspace"
            eventId: "notification"
            autoDelete: true
        }
    }

    compactRepresentation: MouseArea {
        id: compact

        readonly property bool vertical: Plasmoid.formFactor === PlasmaCore.Types.Vertical

        visible: !root.hidden
        Layout.minimumWidth: root.hidden ? 0 : (vertical ? -1 : row.implicitWidth)
        Layout.minimumHeight: root.hidden ? 0 : (vertical ? row.implicitHeight : -1)
        acceptedButtons: Qt.LeftButton | Qt.MiddleButton
        onClicked: mouse => {
            if (mouse.button === Qt.MiddleButton)
                root.refresh();
            else
                root.expanded = !root.expanded;
        }

        GridLayout {
            id: row
            anchors.centerIn: parent
            flow: compact.vertical ? GridLayout.TopToBottom : GridLayout.LeftToRight
            columnSpacing: Kirigami.Units.largeSpacing
            rowSpacing: Kirigami.Units.smallSpacing

            // Waiting, rogctl failing, or nothing connected.
            Kirigami.Icon {
                visible: root.devices.length === 0
                source: "dialog-warning"
                color: root.errorText !== "" ? Kirigami.Theme.negativeTextColor : Kirigami.Theme.textColor
                opacity: root.errorText !== "" ? 1 : 0.6
                Layout.preferredWidth: Kirigami.Units.iconSizes.smallMedium
                Layout.preferredHeight: Kirigami.Units.iconSizes.smallMedium
                Layout.alignment: Qt.AlignCenter
            }

            Repeater {
                model: root.devices

                GridLayout {
                    id: entry

                    required property var modelData
                    readonly property bool hasLevel: modelData.battery !== undefined
                    readonly property color color: modelData.low || modelData.state === "error" ? Kirigami.Theme.negativeTextColor : Kirigami.Theme.textColor

                    flow: compact.vertical ? GridLayout.TopToBottom : GridLayout.LeftToRight
                    columnSpacing: Kirigami.Units.smallSpacing
                    rowSpacing: 0
                    // Asleep: the level shown is the last one read.
                    opacity: modelData.state === "connected" || modelData.state === "error" ? 1 : 0.6
                    Layout.alignment: Qt.AlignCenter

                    Kirigami.Icon {
                        source: root.iconFor(entry.modelData)
                        color: entry.color
                        Layout.preferredWidth: Kirigami.Units.iconSizes.smallMedium
                        Layout.preferredHeight: Kirigami.Units.iconSizes.smallMedium
                        Layout.alignment: Qt.AlignCenter
                    }

                    PlasmaComponents.Label {
                        visible: Plasmoid.configuration.showPercent && entry.hasLevel
                        text: entry.hasLevel ? (compact.vertical ? String(entry.modelData.battery) : entry.modelData.battery + "%") : ""
                        color: entry.color
                        Layout.alignment: Qt.AlignCenter
                    }
                }
            }
        }
    }

    fullRepresentation: PlasmaExtras.Representation {
        Layout.minimumWidth: Kirigami.Units.gridUnit * 14
        Layout.minimumHeight: Kirigami.Units.gridUnit * 6

        ColumnLayout {
            anchors.fill: parent
            anchors.margins: Kirigami.Units.largeSpacing

            Kirigami.Heading {
                level: 3
                text: root.headingText
                Layout.fillWidth: true
                elide: Text.ElideRight
            }

            PlasmaComponents.Label {
                text: root.detailText
                wrapMode: Text.Wrap
                Layout.fillWidth: true
            }

            Item {
                Layout.fillHeight: true
            }

            PlasmaComponents.Button {
                text: i18n("Refresh")
                icon.name: "view-refresh"
                onClicked: root.refresh()
            }
        }
    }
}
