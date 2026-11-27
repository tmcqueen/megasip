import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts

// A coloured banner for an error, warning, success or note, with an optional close button.
QQC2.Control {
    id: msg
    property string text: ""
    // "information", "positive", "warning" or "error"
    property string type: "information"
    property bool showCloseButton: false
    readonly property color accent: type === "error" ? Theme.negativeTextColor
        : type === "warning" ? Theme.neutralTextColor
        : type === "positive" ? Theme.positiveTextColor
        : Theme.highlightColor
    padding: Units.smallSpacing
    leftPadding: Units.largeSpacing

    background: Rectangle {
        radius: 4
        color: Qt.tint(Theme.backgroundColor, Qt.rgba(msg.accent.r, msg.accent.g, msg.accent.b, 0.2))
        border.color: msg.accent
        border.width: 1
    }
    contentItem: RowLayout {
        spacing: Units.largeSpacing
        Icon {
            source: msg.type === "error" ? "dialog-error"
                  : msg.type === "warning" ? "dialog-warning"
                  : msg.type === "positive" ? "dialog-positive"
                  : "dialog-information"
            implicitWidth: Units.iconSizes.smallMedium
        }
        QQC2.Label {
            Layout.fillWidth: true
            text: msg.text
            wrapMode: Text.Wrap
        }
        QQC2.ToolButton {
            visible: msg.showCloseButton
            icon.name: "dialog-close"
            onClicked: msg.visible = false
            QQC2.ToolTip.text: "Close"
            QQC2.ToolTip.visible: hovered
            QQC2.ToolTip.delay: Units.toolTipDelay
        }
    }
}
