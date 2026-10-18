// Your settings for RogMouse.qml. `rogctl configure quickshell` writes this
// file only when it's missing and never overwrites it, so edits here survive
// updates.

import QtQuick

QtObject {
    property string binary: "rogctl"
    property int interval: 60
    // Notify at or below this level; 0 turns it off.
    property int lowThreshold: 20
}
