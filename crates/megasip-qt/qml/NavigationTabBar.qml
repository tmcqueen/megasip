import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts

// The tab bar along the bottom of the main window: one button per action, icon above text.
QQC2.ToolBar {
    id: bar
    property list<QQC2.Action> actions
    position: QQC2.ToolBar.Footer
    contentItem: RowLayout {
        spacing: 0
        Repeater {
            model: bar.actions
            delegate: QQC2.ToolButton {
                required property QQC2.Action modelData
                Layout.fillWidth: true
                Layout.preferredWidth: 1
                action: modelData
                checkable: false
                checked: modelData.checked
                display: QQC2.AbstractButton.TextUnderIcon
                icon.width: Units.iconSizes.smallMedium
                icon.height: Units.iconSizes.smallMedium
            }
        }
    }
}
