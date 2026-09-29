// A minimal bar item for RogMouse.qml: icon and level, red when low, dimmed
// while the mouse sleeps. Click to read the mouse now. Restyle it to match
// your bar; the icons need a Nerd Font.

import QtQuick

Text {
    id: root

    property color normalColor: "#cdd6f4"
    property color lowColor: "#f38ba8"

    readonly property bool error: RogMouse.mouseState === "error"

    readonly property string icon: {
        if (error || RogMouse.mouseState === "disconnected")
            return "󰍾"; // md-mouse_off
        return RogMouse.charging ? "󰂄" : "󰍽"; // md-battery_charging, md-mouse
    }

    visible: RogMouse.status !== null && RogMouse.mouseState !== "disconnected"
    text: RogMouse.battery >= 0 ? `${icon} ${RogMouse.battery}%` : icon
    color: RogMouse.low || error ? lowColor : normalColor
    opacity: RogMouse.connected || error ? 1 : 0.5

    MouseArea {
        anchors.fill: parent
        onClicked: RogMouse.refresh()
    }
}
