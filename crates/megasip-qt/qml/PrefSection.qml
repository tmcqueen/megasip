import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts

// A preferences section: bold title, a line of explanation, controls, then a rule.
ColumnLayout {
    id: sec
    property string title
    property string description
    property bool last: false
    default property alias content: body.data

    Layout.fillWidth: true
    spacing: Units.smallSpacing

    QQC2.Label {
        Layout.fillWidth: true
        text: sec.title
        visible: text.length > 0
        font.weight: Font.Bold
        font.pointSize: Theme.defaultFont.pointSize * 1.05
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
        Layout.topMargin: Units.smallSpacing
        spacing: Units.smallSpacing
    }
    Separator {
        Layout.fillWidth: true
        Layout.topMargin: Units.largeSpacing
        Layout.bottomMargin: Units.largeSpacing
        visible: !sec.last
    }
}
