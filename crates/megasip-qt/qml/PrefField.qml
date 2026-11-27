import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts

// A bold label above a control, with an optional hint underneath.
// Top-aligned so that, in a multi-column GridLayout, labels and controls in
// the same row line up even when only some cells have a hint below them.
ColumnLayout {
    id: f
    property string label
    property string hint
    default property alias content: holder.data

    Layout.fillWidth: true
    Layout.alignment: Qt.AlignTop
    spacing: Units.smallSpacing / 2

    QQC2.Label {
        text: f.label
        font.weight: Font.DemiBold
        visible: text.length > 0
    }
    RowLayout {
        id: holder
        Layout.fillWidth: true
        spacing: Units.smallSpacing
    }
    QQC2.Label {
        Layout.fillWidth: true
        text: f.hint
        visible: text.length > 0
        wrapMode: Text.WordWrap
        font: Theme.smallFont
        opacity: 0.7
    }
}
