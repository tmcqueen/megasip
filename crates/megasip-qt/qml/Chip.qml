import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts

// A small rounded tag with an icon and text.
QQC2.AbstractButton {
    id: chip
    padding: Units.smallSpacing
    leftPadding: Units.largeSpacing
    rightPadding: Units.largeSpacing
    hoverEnabled: true
    background: Rectangle {
        radius: height / 2
        color: Qt.tint(Theme.backgroundColor, Qt.rgba(Theme.textColor.r, Theme.textColor.g, Theme.textColor.b, chip.hovered ? 0.12 : 0.07))
        border.color: Theme.separatorColor
    }
    contentItem: RowLayout {
        spacing: Units.smallSpacing
        Icon {
            visible: chip.icon.name !== ""
            source: chip.icon.name
            implicitWidth: Units.iconSizes.small
        }
        QQC2.Label {
            Layout.fillWidth: true
            text: chip.text
            elide: Text.ElideRight
            font: Theme.smallFont
        }
    }
}
