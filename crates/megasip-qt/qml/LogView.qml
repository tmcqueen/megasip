import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import org.kde.kitemmodels as KItemModels

// Recent log events, filterable by category, level and text.
ColumnLayout {
    id: lv
    required property var phone
    required property var logs
    property var fmtTime
    signal saveRequested()
    spacing: 0

    readonly property var levelRank: ({ ERROR: 1, WARN: 2, INFO: 3, DEBUG: 4, TRACE: 5 })
    readonly property var levelKeys: ["error", "warn", "info", "debug", "trace"]
    function levelColor(l) {
        if (l === "ERROR") return Kirigami.Theme.negativeTextColor
        if (l === "WARN") return Kirigami.Theme.neutralTextColor
        if (l === "INFO") return Kirigami.Theme.textColor
        return Kirigami.Theme.disabledTextColor
    }

    RowLayout {
        Layout.fillWidth: true
        Layout.margins: Kirigami.Units.smallSpacing
        Kirigami.SearchField {
            id: search
            Layout.fillWidth: true
            placeholderText: "Search logs"
            onTextChanged: proxy.invalidateFilter()
        }
        QQC2.ComboBox {
            id: cat
            model: ["SIP and audio", "SIP only", "Audio only"]
            onActivated: proxy.invalidateFilter()
        }
        QQC2.Label { text: "Level" }
        QQC2.ComboBox {
            id: level
            model: ["Errors", "Warnings", "Info", "Debug", "Trace"]
            currentIndex: 2
            onActivated: i => {
                // Capture more detail from now on when asked for it.
                lv.phone.debugSetCapture(lv.levelKeys[Math.max(i, 2)])
                proxy.invalidateFilter()
            }
        }
        QQC2.CheckBox { id: follow; text: "Follow"; checked: true }
        QQC2.Button { text: "Save…"; icon.name: "document-save"; onClicked: lv.saveRequested() }
    }

    QQC2.ScrollView {
        Layout.fillWidth: true
        Layout.fillHeight: true
        ListView {
            id: list
            clip: true
            model: KItemModels.KSortFilterProxyModel {
                id: proxy
                sourceModel: lv.logs
                filterRowCallback: function (row, parent) {
                    const r = lv.logs.get(row)
                    if (cat.currentIndex === 1 && r.category !== "sip") return false
                    if (cat.currentIndex === 2 && r.category !== "audio") return false
                    if ((lv.levelRank[r.level] || 3) > level.currentIndex + 1) return false
                    const t = search.text.trim().toLowerCase()
                    return !t || r.message.toLowerCase().includes(t) || r.target.toLowerCase().includes(t)
                }
            }
            onCountChanged: if (follow.checked) positionViewAtEnd()
            delegate: QQC2.ItemDelegate {
                id: row
                required property var model
                width: ListView.view.width
                onClicked: follow.checked = false
                contentItem: RowLayout {
                    spacing: Kirigami.Units.smallSpacing
                    QQC2.Label {
                        Layout.alignment: Qt.AlignTop
                        text: lv.fmtTime(row.model.t)
                        font: Kirigami.Theme.smallFont
                        opacity: 0.7
                    }
                    QQC2.Label {
                        Layout.alignment: Qt.AlignTop
                        Layout.preferredWidth: Kirigami.Units.gridUnit * 3
                        text: row.model.level
                        color: lv.levelColor(row.model.level)
                        font.family: Kirigami.Theme.fixedWidthFont.family
                        font.bold: row.model.level === "ERROR" || row.model.level === "WARN"
                    }
                    QQC2.Label {
                        Layout.alignment: Qt.AlignTop
                        text: row.model.category === "audio" ? "audio" : "sip"
                        font: Kirigami.Theme.smallFont
                        color: row.model.category === "audio" ? Kirigami.Theme.visitedLinkColor : Kirigami.Theme.linkColor
                    }
                    QQC2.Label {
                        Layout.fillWidth: true
                        text: `<span style="opacity:0.6">${row.model.target}</span>  ${row.model.message.replace(/&/g, "&amp;").replace(/</g, "&lt;")}`
                        textFormat: Text.StyledText
                        wrapMode: Text.Wrap
                    }
                }
            }
            Kirigami.PlaceholderMessage {
                anchors.centerIn: parent
                width: parent.width - Kirigami.Units.gridUnit * 4
                visible: list.count === 0
                text: "Nothing logged yet"
                explanation: "Info and up is kept by default. Pick Debug to capture more from now on."
            }
        }
    }
}
