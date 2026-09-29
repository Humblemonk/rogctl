// Panel widget: reads `rogctl battery --json` every refresh interval. The
// executable data engine only returns output once a command exits, so this
// polls instead of streaming `rogctl watch`. That's also why it keeps the two
// bits of state `rogctl watch` keeps for the other widgets: the last level
// while the mouse sleeps, and whether the low-battery notification was sent.

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
    readonly property int interval: Plasmoid.configuration.interval
    readonly property int lowThreshold: Plasmoid.configuration.lowThreshold

    // The last status line: state, device, battery, charging, error, ...
    property var status: null
    property bool lowNotified: false

    // Not `state`: Item already has one, for QML states.
    readonly property string mouseState: status?.state ?? ""
    readonly property bool hasLevel: status?.battery !== undefined
    // At or below the threshold and not charging; rogctl decides.
    readonly property bool low: status?.low ?? false
    readonly property bool hidden: Plasmoid.configuration.hideWhenDisconnected && mouseState === "disconnected"

    // Solaar's wording, as in the other rogctl widgets.
    function statusWord(s) {
        if (s.charging && s.battery === 100)
            return i18n("full");
        return s.charging ? i18n("recharging") : i18n("discharging");
    }

    function batteryValue(s) {
        if (s.battery === undefined)
            return s.state === "disconnected" ? i18n("unknown") : i18n("offline");
        return i18n("%1% (%2)", s.battery, s.state === "connected" ? statusWord(s) : i18n("offline"));
    }

    readonly property string detailText: {
        if (!status)
            return i18n("Waiting for rogctl…");
        if (mouseState === "error")
            return i18n("Error: %1", status.error || "");
        return i18n("Battery: %1", batteryValue(status));
    }

    readonly property string iconName: {
        if (mouseState === "error" || mouseState === "disconnected")
            return "dialog-warning";
        if (status?.charging && hasLevel)
            return "battery-" + String(Math.round(status.battery / 10) * 10).padStart(3, "0") + "-charging";
        return "input-mouse";
    }

    Plasmoid.icon: iconName
    Plasmoid.status: {
        if (hidden)
            return PlasmaCore.Types.HiddenStatus;
        if (low || mouseState === "error")
            return PlasmaCore.Types.NeedsAttentionStatus;
        return PlasmaCore.Types.ActiveStatus;
    }

    toolTipMainText: status?.device || i18n("ASUS Mouse Battery")
    toolTipSubText: detailText

    function shellQuote(value) {
        return "'" + String(value).replace(/'/g, "'\\''") + "'";
    }

    function refresh() {
        executable.connectSource(shellQuote(binary) + " battery --json --low-threshold " + lowThreshold);
    }

    function handleOutput(exitCode, stdout) {
        let s;
        try {
            s = JSON.parse(stdout.trim());
        } catch (e) {
            // No JSON: rogctl is missing or crashed. 127 is the shell's "not found".
            s = {
                state: "error",
                charging: false,
                error: exitCode === 127 ? i18n("%1 not found on PATH", binary) : i18n("%1 exited with status %2", binary, exitCode)
            };
        }
        // A one-shot read forgets the level once the mouse sleeps; keep showing
        // the last one, as `rogctl watch` does.
        if (s.state === "asleep" && s.battery === undefined && status?.device === s.device) {
            s.battery = status.battery;
            s.battery_updated = status.battery_updated;
            s.low = status.low;
        }
        status = s;
        checkLowBattery(s);
        // Retry an error sooner: the receiver can reject a query for a moment,
        // e.g. right after login.
        pollTimer.interval = (s.state === "error" ? Math.min(10, interval) : interval) * 1000;
        pollTimer.restart();
    }

    // Once per drop to the threshold, re-armed by charging or climbing 5%
    // above it, as `rogctl watch` decides for the other widgets.
    function checkLowBattery(s) {
        if (lowThreshold <= 0 || s.battery === undefined || s.state !== "connected")
            return;
        if (s.charging || s.battery > lowThreshold + 5) {
            lowNotified = false;
        } else if (s.low && !lowNotified) {
            lowNotified = true;
            lowNotification.title = s.device || i18n("Mouse");
            lowNotification.text = i18n("Battery: %1% (%2)", s.battery, statusWord(s));
            lowNotification.sendEvent();
        }
    }

    onIntervalChanged: {
        pollTimer.interval = interval * 1000;
        pollTimer.restart();
    }
    onBinaryChanged: refresh()
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

    Notification {
        id: lowNotification
        componentName: "plasma_workspace"
        eventId: "notification"
        iconName: "input-mouse"
    }

    compactRepresentation: MouseArea {
        id: compact

        readonly property bool vertical: Plasmoid.formFactor === PlasmaCore.Types.Vertical
        readonly property color color: root.low || root.mouseState === "error" ? Kirigami.Theme.negativeTextColor : Kirigami.Theme.textColor

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
            columnSpacing: Kirigami.Units.smallSpacing
            rowSpacing: 0
            // Asleep or gone: the level shown is the last one read.
            opacity: root.mouseState === "connected" || root.mouseState === "error" ? 1 : 0.6

            Kirigami.Icon {
                source: root.iconName
                color: compact.color
                Layout.preferredWidth: Kirigami.Units.iconSizes.smallMedium
                Layout.preferredHeight: Kirigami.Units.iconSizes.smallMedium
                Layout.alignment: Qt.AlignCenter
            }

            PlasmaComponents.Label {
                visible: Plasmoid.configuration.showPercent && root.hasLevel
                text: root.hasLevel ? (compact.vertical ? String(root.status.battery) : root.status.battery + "%") : ""
                color: compact.color
                Layout.alignment: Qt.AlignCenter
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
                text: root.status?.device || i18n("Not connected")
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
