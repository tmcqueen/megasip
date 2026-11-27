import QtQuick
import QtQuick.Controls as QQC2

// A heading label; level 1 is the largest.
QQC2.Label {
    property int level: 1
    font.pointSize: Theme.defaultFont.pointSize * (level <= 1 ? 1.35 : level === 2 ? 1.2 : level === 3 ? 1.15 : 1.1)
    wrapMode: Text.WordWrap
}
