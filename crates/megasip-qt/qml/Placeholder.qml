import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts

// What an empty list or page shows: a faded icon, a line of text, maybe an explanation
// and a button.
ColumnLayout {
    id: ph
    property string text: ""
    property string explanation: ""
    component IconGroup: QtObject { property string name: "" }
    readonly property IconGroup icon: IconGroup {}
    property QQC2.Action helpfulAction: null
    spacing: Units.largeSpacing

    Icon {
        Layout.alignment: Qt.AlignHCenter
        visible: ph.icon.name !== ""
        source: ph.icon.name
        implicitWidth: Units.iconSizes.huge
        implicitHeight: implicitWidth
        opacity: 0.5
    }
    Heading {
        Layout.fillWidth: true
        level: 2
        text: ph.text
        horizontalAlignment: Text.AlignHCenter
        opacity: 0.6
    }
    QQC2.Label {
        Layout.fillWidth: true
        visible: ph.explanation !== ""
        text: ph.explanation
        horizontalAlignment: Text.AlignHCenter
        wrapMode: Text.WordWrap
        opacity: 0.6
    }
    QQC2.Button {
        Layout.alignment: Qt.AlignHCenter
        visible: ph.helpfulAction !== null
        action: ph.helpfulAction
    }
}
