// Bar widget: renders the devices the daemon publishes, an icon and level
// for each. Click for details, right-click to read them now.

import QtQuick
import qs.Common
import qs.Widgets
import qs.Modules.Plugins

PluginComponent {
    id: root

    property var popoutService: null

    PluginGlobalVar {
        id: statusVar
        varName: "status"
        defaultValue: null
    }

    PluginGlobalVar {
        id: refreshVar
        varName: "refresh"
        defaultValue: 0
    }

    readonly property var status: statusVar.value
    readonly property var devices: status?.devices ?? []
    readonly property bool showPercent: pluginData.showPercent ?? true
    readonly property bool hideWhenDisconnected: pluginData.hideWhenDisconnected ?? true
    // An icon while waiting, on an error, or with nothing connected.
    readonly property bool placeholder: !status || status.error !== undefined || devices.length === 0
    readonly property bool shown: !(hideWhenDisconnected && status && status.error === undefined && devices.length === 0)

    onShownChanged: setVisibilityOverride(shown)
    Component.onCompleted: setVisibilityOverride(shown)

    function tr(text) {
        return I18n.trFor("rogMouseBattery", text);
    }

    // Material Symbols names.
    function iconName(s) {
        if (s.state === "error")
            return "error";
        if (s.charging)
            return "battery_charging_full";
        return s.kind === "keyboard" ? "keyboard" : "mouse";
    }

    function percentText(s) {
        return s.battery !== undefined && showPercent ? `${s.battery}%` : "";
    }

    // At or below the threshold and not charging; rogctl decides `low`.
    function contentColor(s) {
        if (s.state === "error" || s.low)
            return Theme.error;
        if (s.state !== "connected")
            return Theme.surfaceVariantText;
        return Theme.widgetIconColor;
    }

    // Status words as the kernel and desktops use them: discharging, charging,
    // full. Solaar says "recharging"; rogctl doesn't.
    function statusWord(s) {
        if (s.charging && s.battery === 100)
            return tr("full");
        return s.charging ? tr("charging") : tr("discharging");
    }

    // "79% (discharging)"; "(offline)" replaces the status while the device sleeps.
    function batteryValue(s) {
        if (s.battery === undefined)
            return tr("offline");
        const word = s.state === "connected" ? statusWord(s) : tr("offline");
        return tr("%1% (%2)").arg(s.battery).arg(word);
    }

    function deviceLine(s) {
        const name = s.device || tr(s.kind === "keyboard" ? "Keyboard" : "Mouse");
        if (s.state === "error")
            return `${name}\n${tr("Error")}: ${s.error || ""}`;
        return `${name}\n${tr("Battery")}: ${batteryValue(s)}`;
    }

    function refresh() {
        refreshVar.set(Date.now());
    }

    pillRightClickAction: () => root.refresh()

    horizontalBarPill: Component {
        Row {
            spacing: Theme.spacingS

            DankIcon {
                visible: root.placeholder
                name: "error"
                size: root.iconSize
                color: root.status?.error !== undefined ? Theme.error : Theme.surfaceVariantText
                anchors.verticalCenter: parent.verticalCenter
            }

            Repeater {
                model: root.devices

                Row {
                    required property var modelData
                    spacing: Theme.spacingXS
                    anchors.verticalCenter: parent.verticalCenter

                    DankIcon {
                        name: root.iconName(modelData)
                        size: root.iconSize
                        color: root.contentColor(modelData)
                        anchors.verticalCenter: parent.verticalCenter
                    }

                    StyledText {
                        visible: text !== ""
                        text: root.percentText(modelData)
                        font.pixelSize: root.textSize
                        color: root.contentColor(modelData)
                        anchors.verticalCenter: parent.verticalCenter
                    }
                }
            }
        }
    }

    verticalBarPill: Component {
        Column {
            spacing: Theme.spacingXS

            DankIcon {
                visible: root.placeholder
                name: "error"
                size: root.iconSize
                color: root.status?.error !== undefined ? Theme.error : Theme.surfaceVariantText
                anchors.horizontalCenter: parent.horizontalCenter
            }

            Repeater {
                model: root.devices

                Column {
                    required property var modelData
                    spacing: 1
                    anchors.horizontalCenter: parent.horizontalCenter

                    DankIcon {
                        name: root.iconName(modelData)
                        size: root.iconSize
                        color: root.contentColor(modelData)
                        anchors.horizontalCenter: parent.horizontalCenter
                    }

                    StyledText {
                        visible: text !== ""
                        text: root.percentText(modelData).replace("%", "")
                        font.pixelSize: root.textSize
                        color: root.contentColor(modelData)
                        anchors.horizontalCenter: parent.horizontalCenter
                    }
                }
            }
        }
    }

    popoutWidth: 320
    popoutHeight: 200 + 50 * Math.max(0, root.devices.length - 1)

    popoutContent: Component {
        PopoutComponent {
            headerText: root.devices.length === 0 ? root.tr("Not connected") : root.devices.length === 1 ? (root.devices[0].device || root.tr("ASUS devices")) : root.tr("ASUS devices")
            detailsText: {
                const s = root.status;
                if (!s)
                    return root.tr("Waiting for rogctl…");
                if (s.error !== undefined)
                    return `${root.tr("Error")}: ${s.error}`;
                if (root.devices.length === 0)
                    return `${root.tr("Battery")}: ${root.tr("unknown")}`;
                if (root.devices.length === 1) {
                    const d = root.devices[0];
                    if (d.state === "error")
                        return `${root.tr("Error")}: ${d.error || ""}`;
                    return `${root.tr("Battery")}: ${root.batteryValue(d)}`;
                }
                return root.devices.map(d => root.deviceLine(d)).join("\n\n");
            }

            DankButton {
                text: root.tr("Refresh")
                iconName: "refresh"
                onClicked: root.refresh()
            }
        }
    }
}
