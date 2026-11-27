import QtQuick
import QtQuick.Controls as QQC2

// A short question with buttons: a title, one line of explanation, standardButtons.
ModalDialog {
    id: dlg
    property string subtitle: ""
    preferredWidth: Units.gridUnit * 22
    padding: Units.largeSpacing * 2
    contentItem: QQC2.Label {
        text: dlg.subtitle
        wrapMode: Text.WordWrap
    }
}
