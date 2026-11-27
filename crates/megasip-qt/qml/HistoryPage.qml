import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts

Item {
    id: page
    required property var phone
    required property var history
    property var accounts: []
    signal dialRequested(string number)
    signal addContact(string name, string number)

    property string filter: "all"
    property var shown: {
        const t = search.text.trim().toLowerCase()
        return history.filter(h => (filter === "all" || h.kind === filter)
            && (!t || (h.name || "").toLowerCase().includes(t) || h.number.toLowerCase().includes(t)))
    }

    function when(ts) {
        const d = new Date(ts * 1000), now = new Date()
        const sameDay = d.toDateString() === now.toDateString()
        const y = new Date(now); y.setDate(now.getDate() - 1)
        const time = d.toLocaleTimeString(Qt.locale(), Locale.ShortFormat)
        if (sameDay) return time
        if (d.toDateString() === y.toDateString()) return "Yesterday " + time
        return d.toLocaleDateString(Qt.locale(), Locale.ShortFormat) + " " + time
    }
    function dur(s) {
        if (!s) return ""
        const m = Math.floor(s / 60), sec = s % 60
        return m + ":" + String(sec).padStart(2, "0")
    }

    ColumnLayout {
        anchors.fill: parent
        spacing: 0

        RowLayout {
            Layout.fillWidth: true
            Layout.margins: Units.smallSpacing
            SearchField { id: search; Layout.fillWidth: true; placeholderText: "Search calls" }
            QQC2.ComboBox {
                model: [{ t: "All", v: "all" }, { t: "Missed", v: "missed" }, { t: "Incoming", v: "in" }, { t: "Outgoing", v: "out" }]
                textRole: "t"
                valueRole: "v"
                onActivated: page.filter = currentValue
            }
            QQC2.ToolButton {
                icon.name: "edit-clear-history"
                enabled: page.history.length > 0
                onClicked: confirmClear.open()
                QQC2.ToolTip.text: "Clear history"
                QQC2.ToolTip.visible: hovered
            }
        }

        QQC2.ScrollView {
            Layout.fillWidth: true
            Layout.fillHeight: true
            ListView {
                id: list
                clip: true
                model: page.shown
                delegate: QQC2.ItemDelegate {
                    id: d
                    required property var modelData
                    width: ListView.view.width
                    onClicked: page.dialRequested(modelData.number)
                    contentItem: RowLayout {
                        Icon {
                            implicitWidth: Units.iconSizes.smallMedium
                            implicitHeight: implicitWidth
                            source: d.modelData.kind === "missed" ? "call-missed" : d.modelData.kind === "in" ? "call-incoming" : "call-outgoing"
                            color: d.modelData.kind === "missed" ? Theme.negativeTextColor : Theme.textColor
                            isMask: d.modelData.kind === "missed"
                        }
                        ColumnLayout {
                            Layout.fillWidth: true
                            spacing: 0
                            QQC2.Label {
                                Layout.fillWidth: true
                                text: d.modelData.name || d.modelData.number
                                color: d.modelData.kind === "missed" ? Theme.negativeTextColor : Theme.textColor
                                elide: Text.ElideRight
                            }
                            QQC2.Label {
                                Layout.fillWidth: true
                                text: (d.modelData.name ? d.modelData.number + "  ·  " : "") + page.when(d.modelData.time)
                                      + (d.modelData.duration ? "  ·  " + page.dur(d.modelData.duration) : "")
                                font: Theme.smallFont
                                opacity: 0.7
                                elide: Text.ElideRight
                            }
                        }
                        QQC2.ToolButton { icon.name: "call-start"; onClicked: page.dialRequested(d.modelData.number) }
                    }
                    TapHandler { acceptedButtons: Qt.RightButton; onTapped: ctx.popup() }
                    onPressAndHold: ctx.popup()
                    QQC2.Menu {
                        id: ctx
                        QQC2.MenuItem { text: "Call"; icon.name: "call-start"; onTriggered: page.dialRequested(d.modelData.number) }
                        QQC2.MenuItem {
                            text: "Add to contacts…"
                            icon.name: "contact-new-symbolic"
                            enabled: !page.phone.contactName(d.modelData.number)
                            onTriggered: page.addContact(d.modelData.name, d.modelData.number)
                        }
                        QQC2.MenuItem { text: "Copy number"; icon.name: "edit-copy"; onTriggered: { clip.text = d.modelData.number; clip.selectAll(); clip.copy() } }
                        QQC2.MenuItem { text: "Delete"; icon.name: "edit-delete-remove"; onTriggered: page.phone.deleteHistory(d.modelData.id) }
                    }
                }
                Placeholder {
                    anchors.centerIn: parent
                    width: parent.width - Units.gridUnit * 4
                    visible: list.count === 0
                    icon.name: "view-history"
                    text: "No calls"
                }
            }
        }
    }
    TextEdit { id: clip; visible: false }

    PromptDialog {
        id: confirmClear
        title: "Clear call history?"
        subtitle: "This removes every call from the list."
        standardButtons: QQC2.Dialog.Ok | QQC2.Dialog.Cancel
        onAccepted: page.phone.clearHistory()
    }
}
