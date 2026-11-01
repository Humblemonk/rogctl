// Your settings for RogMouse.qml. `rogctl configure quickshell` updates this
// file only while it's unchanged, so edits here survive updates.

import QtQuick

QtObject {
    property string binary: "rogctl"
    // Which devices to show: "all", "mouse" or "keyboard".
    property string device: "all"
    property int interval: 60
    // Notify at or below this level; 0 turns it off.
    property int lowThreshold: 20
}
