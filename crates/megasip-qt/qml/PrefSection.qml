import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

// A preferences section: bold title, a line of explanation, controls, then a rule.
ColumnLayout {
    id: sec
    property string title
    property string description
    property bool last: false
    default property alias content: body.data

    Layout.fillWidth: true
    spacing: Kirigami.Units.smallSpacing

    QQC2.Label {
        Layout.fillWidth: true
        text: sec.title
        visible: text.length > 0
        font.weight: Font.Bold
        font.pointSize: Kirigami.Theme.defaultFont.pointSize * 1.05
    }
    QQC2.Label {
        Layout.fillWidth: true
        text: sec.description
        visible: text.length > 0
        wrapMode: Text.WordWrap
        opacity: 0.8
    }
    ColumnLayout {
        id: body
        Layout.fillWidth: true
        Layout.topMargin: Kirigami.Units.smallSpacing
        spacing: Kirigami.Units.smallSpacing
    }
    Kirigami.Separator {
        Layout.fillWidth: true
        Layout.topMargin: Kirigami.Units.largeSpacing
        Layout.bottomMargin: Kirigami.Units.largeSpacing
        visible: !sec.last
    }
}
