import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts

// MicroSIP's ring-in box: who's calling, Answer / Decline.
QQC2.ApplicationWindow {
    id: win
    required property var phone
    required property var calls
    property var accounts: []
    property int callId: 0
    property var call: {
        for (let c of calls) if (c.id === callId) return c
        return calls.length ? calls[0] : null
    }
    property string accountTitle: {
        if (!call) return ""
        for (let a of accounts) if (a.id === call.accountId) return a.title
        return ""
    }

    function showFor(id) {
        callId = id
        win.show()
        win.raise()
        win.requestActivate()
    }

    title: "Incoming call"
    flags: Qt.Dialog | Qt.WindowStaysOnTopHint
    width: Units.gridUnit * 18
    height: Units.gridUnit * 11
    minimumWidth: width
    minimumHeight: height
    maximumWidth: width
    maximumHeight: height
    visible: false
    onCallChanged: if (!call) win.close()

    QQC2.Pane {
        anchors.fill: parent
        contentItem: ColumnLayout {
            spacing: Units.smallSpacing
            RowLayout {
                spacing: Units.largeSpacing
                Avatar {
                    implicitWidth: Units.iconSizes.large
                    implicitHeight: implicitWidth
                    name: win.call ? (win.call.name || win.call.number) : ""
                }
                ColumnLayout {
                    Layout.fillWidth: true
                    spacing: 0
                    Heading {
                        Layout.fillWidth: true
                        level: 2
                        text: win.call ? (win.call.name || win.call.number) : ""
                        elide: Text.ElideRight
                    }
                    QQC2.Label {
                        Layout.fillWidth: true
                        text: win.call && win.call.name ? win.call.number : ""
                        visible: text.length > 0
                        elide: Text.ElideRight
                    }
                    QQC2.Label {
                        Layout.fillWidth: true
                        text: win.accountTitle ? "to " + win.accountTitle : ""
                        visible: text.length > 0
                        font: Theme.smallFont
                        opacity: 0.7
                        elide: Text.ElideRight
                    }
                }
            }
            Item { Layout.fillHeight: true }
            RowLayout {
                Layout.fillWidth: true
                spacing: Units.largeSpacing
                QQC2.Button {
                    Layout.fillWidth: true
                    text: "Answer"
                    icon.name: "call-start"
                    palette.button: Theme.positiveBackgroundColor
                    onClicked: { if (win.call) win.phone.answer(win.call.id); win.close() }
                    focus: true
                }
                QQC2.Button {
                    Layout.fillWidth: true
                    text: "Decline"
                    icon.name: "call-stop"
                    palette.button: Theme.negativeBackgroundColor
                    onClicked: { if (win.call) win.phone.reject(win.call.id); win.close() }
                }
            }
        }
    }
    Shortcut { sequence: "Return"; onActivated: { if (win.call) win.phone.answer(win.call.id); win.close() } }
    Shortcut { sequence: "Escape"; onActivated: { if (win.call) win.phone.reject(win.call.id); win.close() } }
}
