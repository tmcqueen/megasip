import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

// A bold label above a control, with an optional hint underneath.
ColumnLayout {
    id: f
    property string label
    property string hint
    default property alias content: holder.data

    Layout.fillWidth: true
    spacing: Kirigami.Units.smallSpacing / 2

    QQC2.Label {
        text: f.label
        font.weight: Font.DemiBold
        visible: text.length > 0
    }
    RowLayout {
        id: holder
        Layout.fillWidth: true
        spacing: Kirigami.Units.smallSpacing
    }
    QQC2.Label {
        Layout.fillWidth: true
        text: f.hint
        visible: text.length > 0
        wrapMode: Text.WordWrap
        font: Kirigami.Theme.smallFont
        opacity: 0.7
    }
}
