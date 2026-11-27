import QtQuick
import QtQuick.Controls as QQC2

// A password field with a show / hide toggle.
QQC2.TextField {
    id: field
    property bool showPassword: false
    echoMode: showPassword ? TextInput.Normal : TextInput.Password
    inputMethodHints: Qt.ImhHiddenText | Qt.ImhSensitiveData | Qt.ImhNoPredictiveText
    rightPadding: eye.width + Units.smallSpacing
    QQC2.ToolButton {
        id: eye
        anchors { right: parent.right; verticalCenter: parent.verticalCenter }
        height: parent.height - 4
        width: height
        icon.name: field.showPassword ? "password-show-off" : "password-show-on"
        icon.width: Units.iconSizes.small
        icon.height: Units.iconSizes.small
        focusPolicy: Qt.NoFocus
        onClicked: field.showPassword = !field.showPassword
        QQC2.ToolTip.text: field.showPassword ? "Hide password" : "Show password"
        QQC2.ToolTip.visible: hovered
        QQC2.ToolTip.delay: Units.toolTipDelay
    }
}
