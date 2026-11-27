import QtQuick
import QtQuick.Controls as QQC2

// A text field for filtering, with a clear button once there's text.
QQC2.TextField {
    id: field
    rightPadding: clear.visible ? clear.width + Units.smallSpacing : Units.smallSpacing * 2
    inputMethodHints: Qt.ImhNoPredictiveText
    Keys.onEscapePressed: event => {
        if (text) { clear.clicked(); event.accepted = true } else event.accepted = false
    }
    QQC2.ToolButton {
        id: clear
        anchors { right: parent.right; verticalCenter: parent.verticalCenter }
        height: parent.height - 4
        width: height
        visible: field.text !== ""
        icon.name: "edit-clear"
        icon.width: Units.iconSizes.small
        icon.height: Units.iconSizes.small
        focusPolicy: Qt.NoFocus
        onClicked: { field.clear(); field.forceActiveFocus() }
    }
}
