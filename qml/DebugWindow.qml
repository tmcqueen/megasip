import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import QtQuick.Dialogs as Dialogs
import org.kde.kirigami as Kirigami
import org.kde.kitemmodels as KItemModels

// Debug window: SIP trace, call ladder, call quality and logs. Everything is captured in
// memory all the time (bounded), so opening this after a problem still shows it.
QQC2.ApplicationWindow {
    id: dbg
    required property var phone

    title: "MegaSIP — Debug"
    width: Kirigami.Units.gridUnit * 64
    height: Kirigami.Units.gridUnit * 40
    minimumWidth: Kirigami.Units.gridUnit * 40
    minimumHeight: Kirigami.Units.gridUnit * 24
    visible: false
    color: Kirigami.Theme.backgroundColor

    property bool paused: false
    property int lastSip: 0
    property int lastLog: 0
    property int tab: 0
    property var quality: ({ streams: [], last: null, mic: 0, speaker: 0 })
    // Per-stream history for the graphs: id -> { jitter: [], loss: [], buffer: [] }
    property var history: ({})
    property int historyVersion: 0

    signal showMessage(string text)

    function openTab(i) {
        if (i !== undefined) tab = i
        poll()
        pollQuality()
        show()
        raise()
        requestActivate()
    }

    function fmtTime(ms) {
        const d = new Date(ms)
        const p = (n, w) => String(n).padStart(w, "0")
        return `${p(d.getHours(), 2)}:${p(d.getMinutes(), 2)}:${p(d.getSeconds(), 2)}.${p(d.getMilliseconds(), 3)}`
    }
    function statusColor(r) {
        if (r.code === 0) return Kirigami.Theme.textColor
        if (r.code < 200) return Kirigami.Theme.disabledTextColor
        if (r.code < 300) return Kirigami.Theme.positiveTextColor
        if (r.code < 400) return Kirigami.Theme.neutralTextColor
        return Kirigami.Theme.negativeTextColor
    }

    signal cleared()
    onCleared: callLadder.reset()

    // Clear everything the window shows: messages, logs, the ladder, and call quality
    // (live calls' counters start over from now).
    function clearAll() {
        phone.debugClear()
        sipModel.clear(); logModel.clear(); dialogModel.clear()
        dialogIndex = {}
        history = {}
        historyVersion++
        cleared()
        pollQuality()
    }

    function poll() {
        if (paused) return
        const sip = JSON.parse(phone.debugSip(lastSip))
        for (const r of sip) {
            sipModel.append(r)
            lastSip = r.seq
            noteDialog(r)
        }
        if (sipModel.count > 2000) sipModel.remove(0, sipModel.count - 2000)
        const logs = JSON.parse(phone.debugLogs(lastLog))
        for (const r of logs) {
            logModel.append(r)
            lastLog = r.seq
        }
        if (logModel.count > 5000) logModel.remove(0, logModel.count - 5000)
    }
    function pollQuality() {
        const q = JSON.parse(phone.debugQuality())
        const all = q.streams.slice()
        // New arrays each time: QML bindings don't see in-place changes to JS arrays.
        const next = Object.assign({}, history)
        for (const s of all) {
            const h = next[s.id] || { jitter: [], loss: [], buffer: [], mos: [] }
            next[s.id] = {
                jitter: h.jitter.concat([s.jitterMs]).slice(-60),
                loss: h.loss.concat([s.lossPct]).slice(-60),
                buffer: h.buffer.concat([s.bufferMs]).slice(-60),
                mos: h.mos.concat([s.mos || 0]).slice(-60)
            }
        }
        history = next
        historyVersion++
        quality = q
    }

    // Dialogs (one per Call-ID) for the ladder's picker.
    ListModel { id: dialogModel }
    property var dialogIndex: ({})
    function noteDialog(r) {
        if (!r.callId) return
        let i = dialogIndex[r.callId]
        if (i === undefined) {
            const kind = r.method === "REGISTER" ? "Registration" : r.method === "OPTIONS" ? "Options" : r.method === "INVITE" ? "Call" : r.method
            const other = r.dir === "out" ? r.toUser : r.fromUser
            dialogModel.insert(0, { callId: r.callId, kind: kind, other: other || "", t: r.t, count: 1, outcome: "" })
            const idx = {}
            for (let k = 0; k < dialogModel.count; k++) idx[dialogModel.get(k).callId] = k
            dialogIndex = idx
            return
        }
        dialogModel.setProperty(i, "count", dialogModel.get(i).count + 1)
        if (r.code >= 200 && r.method === "INVITE") dialogModel.setProperty(i, "outcome", r.line.replace("SIP/2.0 ", ""))
    }

    ListModel { id: sipModel }
    ListModel { id: logModel }

    Timer { interval: 500; repeat: true; running: dbg.visible; onTriggered: dbg.poll() }
    Timer { interval: 1000; repeat: true; running: dbg.visible; onTriggered: dbg.pollQuality() }

    header: QQC2.ToolBar {
        contentItem: RowLayout {
            QQC2.TabBar {
                id: tabs
                currentIndex: dbg.tab
                onCurrentIndexChanged: dbg.tab = currentIndex
                QQC2.TabButton { text: "SIP trace"; width: implicitWidth }
                QQC2.TabButton { text: "Call ladder"; width: implicitWidth }
                QQC2.TabButton { text: "Call quality"; width: implicitWidth }
                QQC2.TabButton { text: "Logs"; width: implicitWidth }
            }
            Item { Layout.fillWidth: true }
            QQC2.Label {
                text: `${sipModel.count} SIP messages · ${logModel.count} log lines`
                opacity: 0.7
            }
            QQC2.ToolButton {
                icon.name: dbg.paused ? "media-playback-start" : "media-playback-pause"
                text: dbg.paused ? "Resume" : "Pause"
                checkable: true
                checked: dbg.paused
                onToggled: { dbg.paused = checked; if (!checked) dbg.poll() }
            }
            QQC2.ToolButton {
                icon.name: "edit-clear-history"
                text: "Clear"
                onClicked: dbg.clearAll()
            }
        }
    }

    StackLayout {
        anchors.fill: parent
        currentIndex: dbg.tab

        // ============================================================== SIP trace
        ColumnLayout {
            spacing: 0
            RowLayout {
                Layout.fillWidth: true
                Layout.margins: Kirigami.Units.smallSpacing
                Kirigami.SearchField {
                    id: sipSearch
                    Layout.fillWidth: true
                    placeholderText: "Search messages (method, number, header text…)"
                    onTextChanged: sipProxy.invalidateFilter()
                }
                QQC2.CheckBox {
                    id: hideHousekeeping
                    text: "Hide REGISTER / OPTIONS"
                    onToggled: sipProxy.invalidateFilter()
                }
                QQC2.CheckBox {
                    id: follow
                    text: "Follow"
                    checked: true
                }
                QQC2.Button {
                    text: "Save…"
                    icon.name: "document-save"
                    onClicked: { saveDialog.kind = "sip"; saveDialog.open() }
                }
            }
            QQC2.SplitView {
                Layout.fillWidth: true
                Layout.fillHeight: true
                orientation: Qt.Horizontal

                QQC2.ScrollView {
                    QQC2.SplitView.preferredWidth: dbg.width * 0.5
                    QQC2.SplitView.minimumWidth: Kirigami.Units.gridUnit * 18
                    ListView {
                        id: sipList
                        clip: true
                        model: KItemModels.KSortFilterProxyModel {
                            id: sipProxy
                            sourceModel: sipModel
                            filterRowCallback: function (row, parent) {
                                const r = sipModel.get(row)
                                if (hideHousekeeping.checked && (r.method === "REGISTER" || r.method === "OPTIONS")) return false
                                const t = sipSearch.text.trim().toLowerCase()
                                return !t || r.text.toLowerCase().includes(t)
                            }
                        }
                        onCountChanged: if (follow.checked) positionViewAtEnd()
                        highlightMoveDuration: 0
                        delegate: QQC2.ItemDelegate {
                            id: srow
                            required property int index
                            required property var model
                            width: ListView.view.width
                            highlighted: ListView.isCurrentItem
                            onClicked: { sipList.currentIndex = index; sipDetail.text = model.text; follow.checked = false }
                            contentItem: RowLayout {
                                spacing: Kirigami.Units.smallSpacing
                                QQC2.Label {
                                    text: dbg.fmtTime(srow.model.t)
                                    font: Kirigami.Theme.smallFont
                                    opacity: 0.7
                                }
                                QQC2.Label {
                                    text: srow.model.dir === "out" ? "→" : "←"
                                    color: srow.model.dir === "out" ? Kirigami.Theme.linkColor : Kirigami.Theme.visitedLinkColor
                                    font.bold: true
                                }
                                QQC2.Label {
                                    Layout.fillWidth: true
                                    text: srow.model.code ? srow.model.line.replace("SIP/2.0 ", "") + "  (" + srow.model.method + ")"
                                                          : srow.model.method + "  " + (srow.model.toUser || "")
                                    color: dbg.statusColor(srow.model)
                                    font.weight: srow.model.code ? Font.Normal : Font.DemiBold
                                    elide: Text.ElideRight
                                }
                                QQC2.Label {
                                    text: "SDP"
                                    visible: srow.model.sdp
                                    font: Kirigami.Theme.smallFont
                                    color: Kirigami.Theme.neutralTextColor
                                }
                                QQC2.Label {
                                    text: srow.model.transport + " " + srow.model.remote
                                    font: Kirigami.Theme.smallFont
                                    opacity: 0.6
                                }
                            }
                        }
                        Kirigami.PlaceholderMessage {
                            anchors.centerIn: parent
                            width: parent.width - Kirigami.Units.gridUnit * 4
                            visible: sipList.count === 0
                            text: "No SIP messages yet"
                            explanation: "Messages appear here as MegaSIP sends and receives them."
                        }
                    }
                }
                ColumnLayout {
                    spacing: 0
                    RowLayout {
                        Layout.fillWidth: true
                        Layout.margins: Kirigami.Units.smallSpacing
                        QQC2.Label { text: "Message"; font.weight: Font.DemiBold; Layout.fillWidth: true }
                        QQC2.Button {
                            text: "Copy"
                            icon.name: "edit-copy"
                            enabled: sipDetail.text.length > 0
                            onClicked: { sipDetail.selectAll(); sipDetail.copy(); sipDetail.deselect() }
                        }
                    }
                    QQC2.ScrollView {
                        Layout.fillWidth: true
                        Layout.fillHeight: true
                        QQC2.TextArea {
                            id: sipDetail
                            readOnly: true
                            selectByMouse: true
                            wrapMode: TextEdit.NoWrap
                            font.family: Kirigami.Theme.fixedWidthFont.family
                            placeholderText: "Select a message to see it in full."
                            Connections {
                                target: dbg
                                function onShowMessage(text) { sipDetail.text = text; dbg.tab = 0 }
                                function onCleared() { sipDetail.text = ""; sipList.currentIndex = -1 }
                            }
                        }
                    }
                }
            }
        }

        // ============================================================== Call ladder
        CallLadder {
            id: callLadder
            dialogs: dialogModel
            messages: sipModel
            fmtTime: dbg.fmtTime
            statusColor: dbg.statusColor
            onOpenMessage: text => dbg.showMessage(text)
        }

        // ============================================================== Call quality
        CallQuality {
            quality: dbg.quality
            history: dbg.history
            historyVersion: dbg.historyVersion
        }

        // ============================================================== Logs
        LogView {
            phone: dbg.phone
            logs: logModel
            fmtTime: dbg.fmtTime
            onSaveRequested: { saveDialog.kind = "logs"; saveDialog.open() }
        }
    }

    Dialogs.FileDialog {
        id: saveDialog
        property string kind: "sip"
        title: kind === "sip" ? "Save SIP trace" : "Save logs"
        fileMode: Dialogs.FileDialog.SaveFile
        defaultSuffix: "txt"
        nameFilters: ["Text (*.txt *.log)", "All files (*)"]
        onAccepted: {
            let out = ""
            if (kind === "sip") {
                for (let i = 0; i < sipProxy.rowCount(); i++) {
                    const r = sipModel.get(sipProxy.mapToSource(sipProxy.index(i, 0)).row)
                    out += `===== ${dbg.fmtTime(r.t)}  ${r.dir === "out" ? "SENT to" : "RECEIVED from"} ${r.transport} ${r.remote} =====\n${r.text}\n\n`
                }
            } else {
                for (let i = 0; i < logModel.count; i++) {
                    const r = logModel.get(i)
                    out += `${dbg.fmtTime(r.t)} ${r.level.padEnd(5)} [${r.category}] ${r.target}: ${r.message}\n`
                }
            }
            dbg.phone.saveText(selectedFile.toString(), out)
        }
    }

    Shortcut { sequence: "Escape"; onActivated: dbg.close() }
}
