import QtQuick
import QtQuick.Controls as QQC2

// A modal dialog centred on its window.
QQC2.Dialog {
    property real preferredWidth: Units.gridUnit * 20
    parent: QQC2.Overlay.overlay
    anchors.centerIn: parent
    modal: true
    focus: true
    width: Math.min(preferredWidth, parent ? parent.width - Units.gridUnit * 2 : preferredWidth)
}
