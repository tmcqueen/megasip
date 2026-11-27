import QtQuick
import QtQuick.Controls as QQC2

// A bordered panel that groups related content; set contentItem.
QQC2.Pane {
    padding: Units.largeSpacing * 1.5
    background: Rectangle {
        radius: 6
        color: Theme.viewBackgroundColor
        border.color: Theme.separatorColor
    }
}
