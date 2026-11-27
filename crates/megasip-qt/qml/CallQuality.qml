import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts

// Live media statistics per call, with the last minute graphed.
QQC2.ScrollView {
    id: cq
    property var quality: ({ streams: [], last: null, mic: 0, speaker: 0 })
    property var history: ({})
    property int historyVersion: 0
    contentWidth: availableWidth

    // Cards are keyed by stream id and only rebuilt when the set of streams changes, so
    // graphs keep their state between polls.
    property var ids: []
    property bool showingLast: false
    onQualityChanged: {
        const live = quality.streams.map(s => s.id)
        const next = live.length ? live : (quality.last ? [quality.last.id] : [])
        showingLast = live.length === 0 && !!quality.last
        if (next.join(",") !== ids.join(",")) ids = next
    }
    function statsFor(id) {
        for (const s of quality.streams) if (s.id === id) return s
        return quality.last && quality.last.id === id ? quality.last : null
    }

    function mosText(m) {
        if (m === null || m === undefined) return "—"
        return m.toFixed(1)
    }
    function mosWord(m) {
        if (m === null || m === undefined) return "measuring…"
        if (m >= 4.0) return "good"
        if (m >= 3.6) return "fair"
        if (m >= 3.1) return "poor"
        return "bad"
    }
    function mosColor(m) {
        if (m === null || m === undefined) return Theme.disabledTextColor
        if (m >= 4.0) return Theme.positiveTextColor
        if (m >= 3.6) return Theme.neutralTextColor
        return Theme.negativeTextColor
    }
    function kb(b) { return b >= 1048576 ? (b / 1048576).toFixed(1) + " MB" : (b / 1024).toFixed(0) + " KB" }
    function dur(s) { const m = Math.floor(s / 60); return m + ":" + String(Math.floor(s % 60)).padStart(2, "0") }

    ColumnLayout {
        width: cq.availableWidth
        spacing: Units.largeSpacing

        // Levels
        RowLayout {
            Layout.fillWidth: true
            Layout.margins: Units.largeSpacing
            spacing: Units.largeSpacing
            QQC2.Label { text: "Microphone" }
            QQC2.ProgressBar { Layout.fillWidth: true; from: 0; to: 1; value: cq.quality.mic }
            QQC2.Label { text: "Speaker" }
            QQC2.ProgressBar { Layout.fillWidth: true; from: 0; to: 1; value: cq.quality.speaker }
        }

        Placeholder {
            Layout.fillWidth: true
            Layout.topMargin: Units.gridUnit * 2
            visible: cq.ids.length === 0
            icon.name: "call-start"
            text: "No call in progress"
            explanation: "Quality figures appear here during a call, and the last call's stay until the next one."
        }

        Repeater {
            model: cq.ids
            delegate: Card {
                id: card
                required property var modelData
                Layout.fillWidth: true
                Layout.leftMargin: Units.largeSpacing
                Layout.rightMargin: Units.largeSpacing
                visible: s !== null
                readonly property var s: cq.statsFor(modelData) || ({ label: "", codec: "", payloadType: 0, srtp: false, durationS: 0, localPort: 0, remote: "",
                    sending: false, mos: null, rFactor: null, rxPackets: 0, rxBytes: 0, txPackets: 0, txBytes: 0, lost: 0, lossPct: 0, jitterMs: 0,
                    bufferMs: 0, targetBufferMs: 0, lastRxMs: null, concealed: 0, late: 0, duplicates: 0, underruns: 0, driftSkips: 0,
                    overflowMs: 0, srtpFailures: 0, ignored: 0, dtmfRx: 0, dtmfTx: 0, ended: false, id: 0 })
                readonly property var h: { cq.historyVersion; return cq.history[modelData] || { jitter: [], loss: [], buffer: [], mos: [] } }

                contentItem: ColumnLayout {
                    spacing: Units.largeSpacing
                    RowLayout {
                        Layout.fillWidth: true
                        ColumnLayout {
                            Layout.fillWidth: true
                            spacing: 0
                            Heading {
                                level: 3
                                text: card.s.label + (card.s.ended ? "  (ended)" : "")
                            }
                            QQC2.Label {
                                text: `${card.s.codec} (PT ${card.s.payloadType})` + (card.s.srtp ? " · SRTP" : " · unencrypted")
                                      + ` · ${cq.dur(card.s.durationS)}` + ` · local :${card.s.localPort} ↔ ${card.s.remote || "no remote yet"}`
                                      + (!card.s.sending ? " · not sending (hold)" : "")
                                opacity: 0.8
                            }
                        }
                        ColumnLayout {
                            spacing: 0
                            QQC2.Label {
                                Layout.alignment: Qt.AlignHCenter
                                text: cq.mosText(card.s.mos)
                                font.pointSize: Theme.defaultFont.pointSize * 2.2
                                font.bold: true
                                color: cq.mosColor(card.s.mos)
                            }
                            QQC2.Label {
                                Layout.alignment: Qt.AlignHCenter
                                text: "MOS · " + cq.mosWord(card.s.mos)
                                font: Theme.smallFont
                                opacity: 0.8
                                QQC2.ToolTip.text: "Estimated with a simplified E-model (ITU-T G.107) from loss, jitter and buffering. R = "
                                                   + (card.s.rFactor ? card.s.rFactor.toFixed(0) : "—")
                                QQC2.ToolTip.visible: mosHover.hovered
                                HoverHandler { id: mosHover }
                            }
                        }
                    }

                    GridLayout {
                        Layout.fillWidth: true
                        columns: 6
                        columnSpacing: Units.largeSpacing
                        rowSpacing: Units.smallSpacing
                        Repeater {
                            model: [
                                ["Received", `${card.s.rxPackets} pkts · ${cq.kb(card.s.rxBytes)}`],
                                ["Sent", `${card.s.txPackets} pkts · ${cq.kb(card.s.txBytes)}`],
                                ["Lost", `${card.s.lost} (${card.s.lossPct.toFixed(2)}%)`],
                                ["Jitter", `${card.s.jitterMs.toFixed(1)} ms`],
                                ["Playout buffer", `${card.s.bufferMs.toFixed(0)} ms (target ${card.s.targetBufferMs.toFixed(0)})`],
                                ["Last packet", card.s.lastRxMs === null ? "none yet" : `${card.s.lastRxMs} ms ago`],
                                ["Concealed", `${card.s.concealed}`],
                                ["Late / duplicate", `${card.s.late} / ${card.s.duplicates}`],
                                ["Underruns", `${card.s.underruns}`],
                                ["Drift skips / overflow", `${card.s.driftSkips} / ${card.s.overflowMs} ms`],
                                ["SRTP failures", `${card.s.srtpFailures}`],
                                ["Ignored packets", `${card.s.ignored}`],
                                ["DTMF in / out", `${card.s.dtmfRx} / ${card.s.dtmfTx}`],
                                ["Input", cq.quality.source ? `${cq.quality.source} (--source)` : "Microphone"]
                            ]
                            delegate: RowLayout {
                                required property var modelData
                                Layout.columnSpan: 2
                                Layout.fillWidth: true
                                QQC2.Label { text: modelData[0]; opacity: 0.7; Layout.preferredWidth: Units.gridUnit * 8 }
                                QQC2.Label {
                                    text: modelData[1]
                                    font.family: Theme.fixedWidthFont.family
                                    Layout.fillWidth: true
                                    color: (modelData[0] === "SRTP failures" || modelData[0] === "Underruns") && parseInt(modelData[1]) > 0
                                           ? Theme.neutralTextColor : Theme.textColor
                                }
                            }
                        }
                    }

                    RowLayout {
                        Layout.fillWidth: true
                        spacing: Units.largeSpacing
                        Sparkline { Layout.fillWidth: true; title: "Jitter (ms)"; values: card.h.jitter; version: cq.historyVersion; floor: 20 }
                        Sparkline { Layout.fillWidth: true; title: "Loss (%)"; values: card.h.loss; version: cq.historyVersion; floor: 2 }
                        Sparkline { Layout.fillWidth: true; title: "Buffer (ms)"; values: card.h.buffer; version: cq.historyVersion; floor: 120 }
                        Sparkline { Layout.fillWidth: true; title: "MOS"; values: card.h.mos; version: cq.historyVersion; floor: 4.5; fixedMax: true }
                    }
                }
            }
        }
        Item { Layout.preferredHeight: Units.largeSpacing }
    }
}
