import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

// Transfer: to a number (blind), or to another call you're on (attended).
Kirigami.Dialog {
    id: dlg
    property var calls: []
    property var contacts: []
    property int callId: 0
    signal blind(int id, string target)
    signal attended(int id, int other)

    function openFor(id) {
        callId = id
        target.text = ""
        open()
        target.forceActiveFocus()
    }
    property var others: calls.filter(c => c.id !== callId && c.state === "connected")

    title: "Transfer call"
    preferredWidth: Kirigami.Units.gridUnit * 18
    padding: Kirigami.Units.largeSpacing
    standardButtons: Kirigami.Dialog.Cancel
    customFooterActions: [
        Kirigami.Action {
            text: "Transfer"
            icon.name: "go-next"
            enabled: target.text.trim().length > 0
            onTriggered: { dlg.blind(dlg.callId, target.text.trim()); dlg.close() }
        }
    ]

    ColumnLayout {
        spacing: Kirigami.Units.smallSpacing
        QQC2.Label { text: "Send this call to:" }
        QQC2.TextField {
            id: target
            Layout.fillWidth: true
            placeholderText: "Number or SIP address"
            onAccepted: if (text.trim()) { dlg.blind(dlg.callId, text.trim()); dlg.close() }
        }
        Repeater {
            model: {
                const t = target.text.trim().toLowerCase()
                if (t.length < 2) return []
                return dlg.contacts.filter(c => (c.name || "").toLowerCase().includes(t) || c.number.includes(t)).slice(0, 4)
            }
            QQC2.ItemDelegate {
                required property var modelData
                Layout.fillWidth: true
                text: modelData.name + "  ·  " + modelData.number
                onClicked: target.text = modelData.number
            }
        }
        Kirigami.Separator { Layout.fillWidth: true; visible: dlg.others.length > 0 }
        QQC2.Label { text: "Or connect to a call you're on:"; visible: dlg.others.length > 0 }
        Repeater {
            model: dlg.others
            QQC2.ItemDelegate {
                required property var modelData
                Layout.fillWidth: true
                icon.name: "system-users"
                text: (modelData.name || modelData.number)
                onClicked: { dlg.attended(dlg.callId, modelData.id); dlg.close() }
            }
        }
    }
}
