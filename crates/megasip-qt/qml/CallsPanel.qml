import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts

// Active calls (MicroSIP's call tabs): one row per call, plus controls for it.
QQC2.Pane {
    id: panel
    required property var phone
    required property var calls
    property var selected: null
    property real now: 0
    property var formatDuration
    signal transferRequested(int id)
    signal qualityRequested(int id)

    padding: Units.smallSpacing

    background: Rectangle {
        color: Theme.backgroundColor
        Separator { anchors { left: parent.left; right: parent.right; top: parent.top } }
    }

    function stateText(c) {
        if (c.transfer) return c.transfer
        switch (c.state) {
        case "calling": return "Calling…"
        case "ringing": return "Ringing…"
        case "early": return "Connecting…"
        case "incoming": return "Incoming call"
        case "connected":
            if (c.held) return "On hold"
            if (c.remoteHeld) return "Held by other side"
            return formatDuration(now - (c.connectedAt || now))
        }
        return c.reason || c.state
    }

    contentItem: ColumnLayout {
        spacing: Units.smallSpacing

        Repeater {
            model: panel.calls
            delegate: QQC2.ItemDelegate {
                id: row
                required property var modelData
                Layout.fillWidth: true
                highlighted: panel.selected && panel.selected.id === modelData.id && panel.calls.length > 1
                onClicked: if (modelData.state === "connected" && modelData.held) panel.phone.hold(modelData.id, false)
                contentItem: RowLayout {
                    spacing: Units.smallSpacing
                    Icon {
                        source: row.modelData.direction === "in" ? "call-incoming" : "call-outgoing"
                        implicitWidth: Units.iconSizes.smallMedium
                        implicitHeight: implicitWidth
                    }
                    ColumnLayout {
                        Layout.fillWidth: true
                        spacing: 0
                        QQC2.Label {
                            Layout.fillWidth: true
                            text: row.modelData.name || row.modelData.number
                            font.weight: Font.DemiBold
                            elide: Text.ElideRight
                        }
                        QQC2.Label {
                            Layout.fillWidth: true
                            text: panel.stateText(row.modelData)
                                  + (row.modelData.codec ? "  ·  " + row.modelData.codec : "")
                            font: Theme.smallFont
                            opacity: 0.75
                            elide: Text.ElideRight
                        }
                    }
                    Icon {
                        source: "security-high"
                        visible: row.modelData.secure
                        implicitWidth: Units.iconSizes.small
                        implicitHeight: implicitWidth
                        QQC2.ToolTip.text: "Encrypted media (SRTP)"
                    }
                    QQC2.ToolButton {
                        visible: row.modelData.state === "incoming"
                        icon.name: "call-start"
                        icon.color: Theme.positiveTextColor
                        onClicked: panel.phone.answer(row.modelData.id)
                        QQC2.ToolTip.text: "Answer"
                        QQC2.ToolTip.visible: hovered
                    }
                    QQC2.ToolButton {
                        visible: row.modelData.state === "connected" || row.modelData.state === "early"
                        icon.name: "help-about"
                        onClicked: panel.qualityRequested(row.modelData.id)
                        QQC2.ToolTip.text: "Call quality and SIP trace (Ctrl+Shift+D)"
                        QQC2.ToolTip.visible: hovered
                    }
                    QQC2.ToolButton {
                        visible: row.modelData.state === "connected"
                        icon.name: row.modelData.held ? "media-playback-start" : "media-playback-pause"
                        checkable: true
                        checked: row.modelData.held
                        onClicked: panel.phone.hold(row.modelData.id, !row.modelData.held)
                        QQC2.ToolTip.text: row.modelData.held ? "Resume" : "Hold"
                        QQC2.ToolTip.visible: hovered
                    }
                    QQC2.ToolButton {
                        icon.name: "call-stop"
                        icon.color: Theme.negativeTextColor
                        onClicked: row.modelData.state === "incoming" ? panel.phone.reject(row.modelData.id) : panel.phone.hangup(row.modelData.id)
                        QQC2.ToolTip.text: row.modelData.state === "incoming" ? "Decline" : "Hang up"
                        QQC2.ToolTip.visible: hovered
                    }
                }
            }
        }

        // Controls for the active call.
        RowLayout {
            Layout.alignment: Qt.AlignHCenter
            visible: panel.calls.some(c => c.state === "connected")
            spacing: Units.smallSpacing

            QQC2.ToolButton {
                icon.name: "mic-off"
                text: "Mute"
                checkable: true
                checked: panel.phone.muted
                onToggled: panel.phone.setMuteOn(checked)
                display: QQC2.AbstractButton.TextUnderIcon
            }
            QQC2.ToolButton {
                icon.name: "go-next"
                text: "Transfer"
                enabled: panel.selected && panel.selected.state === "connected"
                onClicked: panel.transferRequested(panel.selected.id)
                display: QQC2.AbstractButton.TextUnderIcon
            }
            QQC2.ToolButton {
                icon.name: "system-users"
                text: "Conference"
                checkable: true
                checked: panel.phone.conference
                enabled: panel.calls.filter(c => c.state === "connected").length > 1 || panel.phone.conference
                onToggled: panel.phone.setConferenceOn(checked)
                display: QQC2.AbstractButton.TextUnderIcon
            }
            QQC2.ToolButton {
                icon.name: "call-stop"
                text: "End all"
                visible: panel.calls.length > 1
                onClicked: panel.phone.hangupAll()
                display: QQC2.AbstractButton.TextUnderIcon
            }
        }

        // Microphone (or --source) level, so you can see you're being heard.
        RowLayout {
            Layout.fillWidth: true
            Layout.leftMargin: Units.largeSpacing
            Layout.rightMargin: Units.largeSpacing
            visible: panel.calls.some(c => c.state === "connected")
            Icon {
                source: panel.phone.sourceLabel ? "audio-x-generic" : "audio-input-microphone"
                implicitWidth: Units.iconSizes.small
                implicitHeight: Units.iconSizes.small
            }
            QQC2.Label {
                visible: panel.phone.sourceLabel !== ""
                text: panel.phone.sourceLabel
                font: Theme.smallFont
                elide: Text.ElideMiddle
                Layout.maximumWidth: Units.gridUnit * 7
            }
            QQC2.ProgressBar {
                Layout.fillWidth: true
                from: 0
                to: 1
                value: 0
                Timer {
                    interval: 100
                    repeat: true
                    running: parent.visible
                    onTriggered: parent.value = panel.phone.micLevel()
                }
            }
        }
    }
}
