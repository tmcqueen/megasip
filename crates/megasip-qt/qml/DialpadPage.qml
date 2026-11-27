import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts

// MicroSIP's dial pad: number field, keypad, call / redial / hang up.
// During a call the keys send DTMF to the active call.
Item {
    id: page
    required property var phone
    property var activeCall: null
    property var settings: ({})
    property var contacts: []
    property var history: []
    signal dialRequested(string number)
    readonly property var training: phone.trainingJson ? JSON.parse(phone.trainingJson) : ({})

    readonly property bool inCall: activeCall !== null && (activeCall.state === "connected" || activeCall.state === "early")

    function press(d) {
        if (inCall) {
            phone.dtmf(activeCall.id, d)
            dtmfEcho.text = (dtmfEcho.text + d).slice(-24)
        } else {
            number.insert(number.cursorPosition, d)
            number.forceActiveFocus()
        }
    }
    function call() {
        if (number.text.trim().length) {
            page.dialRequested(number.text.trim())
            number.text = ""
        } else if (page.settings.lastNumber) {
            number.text = page.settings.lastNumber   // MicroSIP: first press recalls, second dials
        }
    }

    // Suggestions from contacts and history as you type.
    property var suggestions: {
        const t = number.text.trim().toLowerCase()
        if (t.length < 2 || inCall) return []
        const out = [], seen = {}
        for (let c of contacts) {
            if ((c.name && c.name.toLowerCase().includes(t)) || c.number.toLowerCase().includes(t)) {
                if (!seen[c.number]) { seen[c.number] = 1; out.push({ name: c.name, number: c.number }) }
            }
            if (out.length >= 5) break
        }
        for (let h of history) {
            if (out.length >= 5) break
            if (h.number.toLowerCase().includes(t) && !seen[h.number]) { seen[h.number] = 1; out.push({ name: h.name, number: h.number }) }
        }
        return out
    }

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: Units.largeSpacing
        spacing: Units.smallSpacing

        RowLayout {
            Layout.fillWidth: true
            visible: !page.inCall
            QQC2.TextField {
                id: number
                Layout.fillWidth: true
                font.pointSize: Theme.defaultFont.pointSize * 1.5
                horizontalAlignment: Text.AlignHCenter
                placeholderText: "Number or SIP address"
                inputMethodHints: Qt.ImhDialableCharactersOnly
                onAccepted: page.call()
                Keys.onEscapePressed: text = ""
                focus: true
            }
            QQC2.ToolButton {
                icon.name: "edit-clear-locationbar-rtl"
                enabled: number.text.length > 0
                onClicked: number.remove(number.cursorPosition - 1, number.cursorPosition)
                onPressAndHold: number.text = ""
                QQC2.ToolTip.text: "Backspace (hold to clear)"
                QQC2.ToolTip.visible: hovered
            }
        }

        ColumnLayout {
            Layout.fillWidth: true
            visible: page.inCall
            spacing: 0
            QQC2.Label {
                Layout.fillWidth: true
                horizontalAlignment: Text.AlignHCenter
                text: page.activeCall ? (page.activeCall.name || page.activeCall.number) : ""
                font.pointSize: Theme.defaultFont.pointSize * 1.3
                elide: Text.ElideRight
            }
            QQC2.Label {
                id: dtmfEcho
                Layout.fillWidth: true
                horizontalAlignment: Text.AlignHCenter
                font.family: "monospace"
                opacity: 0.7
                text: ""
                Connections {
                    target: page
                    function onActiveCallChanged() { if (!page.inCall) dtmfEcho.text = "" }
                }
            }
        }

        ListView {
            Layout.fillWidth: true
            Layout.preferredHeight: contentHeight
            visible: page.suggestions.length > 0 && number.activeFocus
            interactive: false
            model: page.suggestions
            delegate: QQC2.ItemDelegate {
                required property var modelData
                width: ListView.view.width
                text: modelData.name ? `${modelData.name}  ·  ${modelData.number}` : modelData.number
                onClicked: { number.text = modelData.number; page.call() }
            }
        }

        // The space between the number and the keys; a connected agent's Agent Training
        // board shows here while it has one open.
        Item {
            Layout.fillHeight: true
            Layout.fillWidth: true
            TrainingBoard {
                anchors.fill: parent
                visible: page.training.visible === true
                model: page.training
            }
        }

        GridLayout {
            id: keys
            Layout.alignment: Qt.AlignHCenter
            Layout.fillWidth: true
            Layout.maximumWidth: Units.gridUnit * 16
            columns: 3
            rowSpacing: Units.smallSpacing
            columnSpacing: Units.smallSpacing
            Repeater {
                model: [["1", ""], ["2", "ABC"], ["3", "DEF"], ["4", "GHI"], ["5", "JKL"], ["6", "MNO"],
                        ["7", "PQRS"], ["8", "TUV"], ["9", "WXYZ"], ["*", ""], ["0", "+"], ["#", ""]]
                QQC2.Button {
                    required property var modelData
                    Layout.fillWidth: true
                    Layout.preferredHeight: Units.gridUnit * 2.8
                    focusPolicy: Qt.NoFocus
                    contentItem: ColumnLayout {
                        spacing: 0
                        QQC2.Label {
                            Layout.alignment: Qt.AlignHCenter
                            text: modelData[0]
                            font.pointSize: Theme.defaultFont.pointSize * 1.6
                        }
                        QQC2.Label {
                            Layout.alignment: Qt.AlignHCenter
                            text: modelData[1]
                            font: Theme.smallFont
                            opacity: 0.6
                            visible: text.length > 0
                        }
                    }
                    onClicked: page.press(modelData[0])
                    onPressAndHold: if (modelData[0] === "0" && !page.inCall) page.press("+")
                }
            }
        }

        RowLayout {
            Layout.alignment: Qt.AlignHCenter
            Layout.topMargin: Units.smallSpacing
            spacing: Units.largeSpacing
            visible: !page.inCall

            QQC2.Button {
                icon.name: "edit-undo"
                text: "Redial"
                display: QQC2.AbstractButton.IconOnly
                enabled: !!page.settings.lastNumber
                onClicked: page.phone.redial()
                QQC2.ToolTip.text: "Redial " + (page.settings.lastNumber || "")
                QQC2.ToolTip.visible: hovered
                focusPolicy: Qt.NoFocus
            }
            QQC2.RoundButton {
                implicitWidth: Units.gridUnit * 3.5
                implicitHeight: implicitWidth
                icon.name: "call-start"
                icon.width: Units.iconSizes.medium
                icon.height: Units.iconSizes.medium
                icon.color: "white"
                palette.button: Theme.positiveTextColor
                onClicked: page.call()
                QQC2.ToolTip.text: "Call"
                QQC2.ToolTip.visible: hovered
                focusPolicy: Qt.NoFocus
            }
            QQC2.Button {
                icon.name: "irc-voice"
                text: "Voicemail"
                display: QQC2.AbstractButton.IconOnly
                enabled: page.voicemailNumber.length > 0
                onClicked: page.dialRequested(page.voicemailNumber)
                QQC2.ToolTip.text: "Voicemail"
                QQC2.ToolTip.visible: hovered
                focusPolicy: Qt.NoFocus
            }
        }
    }

    property string voicemailNumber: {
        const accs = settings.accounts || []
        for (let a of accs) if (a.id === phone.activeAccount && a.voicemailNumber) return a.voicemailNumber
        return ""
    }

    // Typing digits anywhere on the page goes to the number (or DTMF in a call).
    Keys.onPressed: event => {
        const t = event.text
        if (t.length === 1 && "0123456789*#+".includes(t)) {
            if (page.inCall) { page.press(t); event.accepted = true }
        }
    }
    focus: true
}
