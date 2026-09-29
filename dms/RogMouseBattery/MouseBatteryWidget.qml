// Bar widget: renders the status the daemon publishes. Click for details,
// right-click to read the mouse now.

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
    readonly property bool showPercent: pluginData.showPercent ?? true
    readonly property bool hideWhenDisconnected: pluginData.hideWhenDisconnected ?? true
    // At or below the threshold and not charging; rogctl decides.
    readonly property bool low: status?.low ?? false
    // Material Symbols names.
    readonly property string iconName: {
        if (status?.state === "error" || status?.state === "disconnected")
            return "error";
        return status?.charging ? "battery_charging_full" : "mouse";
    }
    readonly property string percentText: status?.battery !== undefined && showPercent ? `${status.battery}%` : ""
    readonly property color contentColor: {
        if (status?.state === "error" || low)
            return Theme.error;
        if (status?.state !== "connected")
            return Theme.surfaceVariantText;
        return Theme.widgetIconColor;
    }
    readonly property bool shown: !(hideWhenDisconnected && status?.state === "disconnected")

    onShownChanged: setVisibilityOverride(shown)
    Component.onCompleted: setVisibilityOverride(shown)

    function tr(text) {
        return I18n.trFor("rogMouseBattery", text);
    }

    // Solaar's status words: discharging, recharging, full.
    function statusWord(s) {
        if (s.charging && s.battery === 100)
            return tr("full");
        return s.charging ? tr("recharging") : tr("discharging");
    }

    // "79% (discharging)"; "(offline)" replaces the status while the mouse sleeps.
    function batteryValue(s) {
        if (s.battery === undefined)
            return s.state === "disconnected" ? tr("unknown") : tr("offline");
        const word = s.state === "connected" ? statusWord(s) : tr("offline");
        return tr("%1% (%2)").arg(s.battery).arg(word);
    }

    function refresh() {
        refreshVar.set(Date.now());
    }

    pillRightClickAction: () => root.refresh()

    horizontalBarPill: Component {
        Row {
            spacing: Theme.spacingXS

            DankIcon {
                name: root.iconName
                size: root.iconSize
                color: root.contentColor
                anchors.verticalCenter: parent.verticalCenter
            }

            StyledText {
                visible: text !== ""
                text: root.percentText
                font.pixelSize: root.textSize
                color: root.contentColor
                anchors.verticalCenter: parent.verticalCenter
            }
        }
    }

    verticalBarPill: Component {
        Column {
            spacing: 1

            DankIcon {
                name: root.iconName
                size: root.iconSize
                color: root.contentColor
                anchors.horizontalCenter: parent.horizontalCenter
            }

            StyledText {
                visible: text !== ""
                text: root.percentText.replace("%", "")
                font.pixelSize: root.textSize
                color: root.contentColor
                anchors.horizontalCenter: parent.horizontalCenter
            }
        }
    }

    popoutWidth: 320
    popoutHeight: 200

    popoutContent: Component {
        PopoutComponent {
            headerText: root.status?.device || root.tr("Not connected")
            detailsText: {
                const s = root.status;
                if (!s)
                    return root.tr("Waiting for rogctl…");
                if (s.state === "error")
                    return `${root.tr("Error")}: ${s.error || ""}`;
                return `${root.tr("Battery")}: ${root.batteryValue(s)}`;
            }

            DankButton {
                text: root.tr("Refresh")
                iconName: "refresh"
                onClicked: root.refresh()
            }
        }
    }
}
