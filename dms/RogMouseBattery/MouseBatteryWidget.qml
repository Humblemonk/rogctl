// Bar widget: renders the status the daemon publishes. Click for details,
// right-click to read the mouse now.

import QtQuick
import qs.Common
import qs.Widgets
import qs.Modules.Plugins
import "battery.js" as Battery

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
    readonly property int lowThreshold: pluginData.lowThreshold ?? 20

    readonly property bool low: status !== null && status.battery !== undefined && !status.charging
        && lowThreshold > 0 && status.battery <= lowThreshold
    readonly property string iconName: status ? Battery.icon(status) : "mouse"
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
                return `${root.tr("Battery")}: ${Battery.value(s, root.tr)}`;
            }

            DankButton {
                text: root.tr("Refresh")
                iconName: "refresh"
                onClicked: root.refresh()
            }
        }
    }
}
