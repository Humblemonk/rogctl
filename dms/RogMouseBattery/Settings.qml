import QtQuick
import qs.Common
import qs.Modules.Plugins

PluginSettings {
    pluginId: "rogMouseBattery"

    StringSetting {
        settingKey: "binary"
        label: I18n.trFor("rogMouseBattery", "rogctl command")
        description: I18n.trFor("rogMouseBattery", "Name or full path of the rogctl binary.")
        placeholder: "rogctl"
        defaultValue: "rogctl"
    }

    SelectionSetting {
        settingKey: "device"
        label: I18n.trFor("rogMouseBattery", "Show")
        description: I18n.trFor("rogMouseBattery", "Which ASUS devices to show the battery of.")
        options: [
            {
                label: I18n.trFor("rogMouseBattery", "Mouse and keyboard"),
                value: "all"
            },
            {
                label: I18n.trFor("rogMouseBattery", "Mouse"),
                value: "mouse"
            },
            {
                label: I18n.trFor("rogMouseBattery", "Keyboard"),
                value: "keyboard"
            }
        ]
        defaultValue: "all"
    }

    SliderSetting {
        settingKey: "interval"
        label: I18n.trFor("rogMouseBattery", "Refresh seconds")
        description: I18n.trFor("rogMouseBattery", "How often to read the battery.")
        defaultValue: 60
        minimum: 5
        maximum: 600
        unit: "s"
    }

    SliderSetting {
        settingKey: "lowThreshold"
        label: I18n.trFor("rogMouseBattery", "Low battery warning (%)")
        description: I18n.trFor("rogMouseBattery", "Notify and turn the widget red at or below this level. 0 disables it.")
        defaultValue: 20
        minimum: 0
        maximum: 100
        unit: "%"
    }

    ToggleSetting {
        settingKey: "showPercent"
        label: I18n.trFor("rogMouseBattery", "Show percentage")
        defaultValue: true
    }

    ToggleSetting {
        settingKey: "hideWhenDisconnected"
        label: I18n.trFor("rogMouseBattery", "Hide when nothing is connected")
        description: I18n.trFor("rogMouseBattery", "Hide the widget while the receiver or cable is unplugged.")
        defaultValue: true
    }
}
