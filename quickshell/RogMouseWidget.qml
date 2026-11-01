// A minimal bar item for RogMouse.qml: an icon and level for each device,
// red when low, dimmed while it sleeps. Click to read them now. Restyle it to
// match your bar; the icons need a Nerd Font.

import QtQuick

Item {
    id: root

    property color normalColor: "#cdd6f4"
    property color lowColor: "#f38ba8"

    readonly property bool failed: RogMouse.error !== ""

    implicitWidth: row.implicitWidth
    implicitHeight: row.implicitHeight
    visible: RogMouse.report !== null && (failed || RogMouse.devices.length > 0)

    Row {
        id: row
        spacing: 8

        // rogctl couldn't run.
        Text {
            visible: root.failed
            text: "󰍾" // md-mouse_off
            color: root.lowColor
        }

        Repeater {
            model: RogMouse.devices

            Text {
                required property var modelData
                readonly property bool error: modelData.state === "error"
                readonly property bool keyboard: modelData.kind === "keyboard"
                readonly property string icon: {
                    if (error)
                        return keyboard ? "󰌐" : "󰍾"; // md-keyboard_off, md-mouse_off
                    if (modelData.charging)
                        return "󰂄"; // md-battery_charging
                    return keyboard ? "󰌌" : "󰍽"; // md-keyboard, md-mouse
                }

                text: modelData.battery !== undefined ? `${icon} ${modelData.battery}%` : icon
                color: modelData.low || error ? root.lowColor : root.normalColor
                opacity: modelData.state === "connected" || error ? 1 : 0.5
            }
        }
    }

    MouseArea {
        anchors.fill: parent
        onClicked: RogMouse.refresh()
    }
}
