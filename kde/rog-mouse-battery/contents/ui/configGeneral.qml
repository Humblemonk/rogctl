import QtQuick
import QtQuick.Controls as QQC2
import org.kde.kirigami as Kirigami
import org.kde.kcmutils as KCM

KCM.SimpleKCM {
    property alias cfg_binary: binaryField.text
    property alias cfg_interval: intervalSpin.value
    property alias cfg_lowThreshold: thresholdSpin.value
    property alias cfg_showPercent: showPercentBox.checked
    property alias cfg_hideWhenDisconnected: hideBox.checked

    Kirigami.FormLayout {
        QQC2.TextField {
            id: binaryField
            Kirigami.FormData.label: i18n("rogctl command:")
            placeholderText: "rogctl"
        }

        QQC2.SpinBox {
            id: intervalSpin
            Kirigami.FormData.label: i18n("Refresh seconds:")
            from: 5
            to: 600
        }

        QQC2.SpinBox {
            id: thresholdSpin
            Kirigami.FormData.label: i18n("Low battery warning (%):")
            from: 0
            to: 100
        }

        QQC2.Label {
            text: i18n("Notify and turn the widget red at or below this level. 0 disables it.")
            wrapMode: Text.Wrap
            font: Kirigami.Theme.smallFont
        }

        QQC2.CheckBox {
            id: showPercentBox
            Kirigami.FormData.label: i18n("Panel:")
            text: i18n("Show percentage")
        }

        QQC2.CheckBox {
            id: hideBox
            text: i18n("Hide when no mouse is connected")
        }
    }
}
