import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

// Sequence diagram of one dialog (Call-ID): MegaSIP on the left, each remote address it
// talked to as its own lane, one arrow per message.
Item {
    id: ladder
    required property var dialogs
    required property var messages
    property var fmtTime
    property var statusColor
    signal openMessage(string text)

    property string callId: ""
    property var rows: []
    property var lanes: ["MegaSIP"]
    property int selectedRow: -1

    function rebuild() {
        if (!callId) { rows = []; lanes = ["MegaSIP"]; return }
        const out = [], ln = ["MegaSIP"]
        for (let i = 0; i < messages.count; i++) {
            const r = messages.get(i)
            if (r.callId !== callId) continue
            if (!ln.includes(r.remote)) ln.push(r.remote)
            out.push({ t: r.t, dir: r.dir, remote: r.remote, transport: r.transport, line: r.line, method: r.method,
                       code: r.code, sdp: r.sdp, text: r.text, cseq: r.cseq })
        }
        lanes = ln
        rows = out
    }
    Connections {
        target: ladder.messages
        function onCountChanged() { ladder.rebuild() }
    }
    // Show the newest call until one is picked.
    property bool userPicked: false
    Connections {
        target: ladder.dialogs
        function onCountChanged() { ladder.autoPick() }
    }
    function autoPick() {
        if (userPicked) return
        for (let i = 0; i < dialogs.count; i++) {
            const d = dialogs.get(i)
            if (d.kind === "Call") { callId = d.callId; return }
        }
    }
    Component.onCompleted: autoPick()
    onCallIdChanged: { selectedRow = -1; rebuild() }

    function label(r) {
        if (r.code) return r.line.replace("SIP/2.0 ", "")
        return r.method + (r.sdp ? " (SDP)" : "")
    }

    QQC2.SplitView {
        anchors.fill: parent

        // ---------------------------------------------------------- dialog picker
        ColumnLayout {
            QQC2.SplitView.preferredWidth: Kirigami.Units.gridUnit * 17
            QQC2.SplitView.minimumWidth: Kirigami.Units.gridUnit * 12
            spacing: 0
            QQC2.CheckBox {
                id: callsOnly
                Layout.margins: Kirigami.Units.smallSpacing
                text: "Calls only"
                checked: true
            }
            QQC2.ScrollView {
                Layout.fillWidth: true
                Layout.fillHeight: true
                ListView {
                    id: dlist
                    clip: true
                    model: ladder.dialogs
                    delegate: QQC2.ItemDelegate {
                        id: drow
                        required property var model
                        width: ListView.view.width
                        visible: !callsOnly.checked || model.kind === "Call"
                        height: visible ? implicitHeight : 0
                        highlighted: model.callId === ladder.callId
                        onClicked: { ladder.userPicked = true; ladder.callId = model.callId }
                        contentItem: ColumnLayout {
                            spacing: 0
                            QQC2.Label {
                                Layout.fillWidth: true
                                text: drow.model.kind + (drow.model.other ? " · " + drow.model.other : "")
                                font.weight: Font.DemiBold
                                elide: Text.ElideRight
                            }
                            QQC2.Label {
                                Layout.fillWidth: true
                                text: ladder.fmtTime(drow.model.t).slice(0, 8) + " · " + drow.model.count + " messages"
                                      + (drow.model.outcome ? " · " + drow.model.outcome : "")
                                font: Kirigami.Theme.smallFont
                                opacity: 0.7
                                elide: Text.ElideRight
                            }
                        }
                    }
                }
            }
        }

        // ---------------------------------------------------------- the ladder
        ColumnLayout {
            spacing: 0
            Kirigami.PlaceholderMessage {
                Layout.fillWidth: true
                Layout.fillHeight: true
                visible: ladder.rows.length === 0
                icon.name: "view-list-details"
                text: ladder.callId ? "No messages" : "Pick a call on the left"
                explanation: "Each arrow is one SIP message. Click one to read it."
            }

            // Lane headers
            Item {
                id: lanesHeader
                Layout.fillWidth: true
                Layout.preferredHeight: Kirigami.Units.gridUnit * 2
                visible: ladder.rows.length > 0
                Repeater {
                    model: ladder.lanes
                    QQC2.Label {
                        required property int index
                        required property var modelData
                        x: ladderArea.laneX(index) - width / 2
                        anchors.verticalCenter: parent.verticalCenter
                        text: modelData
                        font.weight: Font.Bold
                        horizontalAlignment: Text.AlignHCenter
                    }
                }
            }
            Kirigami.Separator { Layout.fillWidth: true; visible: ladder.rows.length > 0 }

            QQC2.ScrollView {
                id: ladderArea
                Layout.fillWidth: true
                Layout.fillHeight: true
                visible: ladder.rows.length > 0
                readonly property real timeCol: Kirigami.Units.gridUnit * 8
                function laneX(i) {
                    const n = ladder.lanes.length
                    const left = timeCol + Kirigami.Units.gridUnit * 3
                    const right = width - Kirigami.Units.gridUnit * 3
                    return n <= 1 ? left : left + i * (right - left) / (n - 1)
                }

                ListView {
                    id: lview
                    clip: true
                    model: ladder.rows
                    delegate: Item {
                        id: lrow
                        required property int index
                        required property var modelData
                        width: ListView.view.width
                        height: Kirigami.Units.gridUnit * 2.2
                        readonly property int remoteLane: Math.max(1, ladder.lanes.indexOf(modelData.remote))
                        readonly property real x0: ladderArea.laneX(modelData.dir === "out" ? 0 : remoteLane)
                        readonly property real x1: ladderArea.laneX(modelData.dir === "out" ? remoteLane : 0)
                        readonly property color tint: ladder.statusColor(modelData)

                        Rectangle {
                            anchors.fill: parent
                            color: Kirigami.Theme.highlightColor
                            opacity: ladder.selectedRow === lrow.index ? 0.25 : (hover.hovered ? 0.1 : 0)
                        }
                        HoverHandler { id: hover }
                        TapHandler {
                            onTapped: { ladder.selectedRow = lrow.index; ladder.openMessage(lrow.modelData.text) }
                        }

                        // Time column: clock time and the gap since the previous message.
                        Column {
                            x: Kirigami.Units.smallSpacing
                            anchors.verticalCenter: parent.verticalCenter
                            QQC2.Label {
                                text: ladder.fmtTime(lrow.modelData.t)
                                font: Kirigami.Theme.smallFont
                            }
                            QQC2.Label {
                                readonly property int gap: lrow.index > 0 ? lrow.modelData.t - ladder.rows[lrow.index - 1].t : 0
                                text: lrow.index > 0 ? "+" + (gap >= 1000 ? (gap / 1000).toFixed(2) + " s" : gap + " ms") : ""
                                font: Kirigami.Theme.smallFont
                                color: gap >= 2000 ? Kirigami.Theme.neutralTextColor : Kirigami.Theme.disabledTextColor
                            }
                        }

                        // Lifelines
                        Repeater {
                            model: ladder.lanes.length
                            Rectangle {
                                required property int index
                                x: ladderArea.laneX(index) - 0.5
                                width: 1
                                height: lrow.height
                                color: Kirigami.Theme.disabledTextColor
                                opacity: 0.6
                            }
                        }

                        // The arrow: a line and a head, drawn with plain items so it shows
                        // up immediately.
                        Rectangle {
                            readonly property real y0: lrow.height * 0.68
                            x: Math.min(lrow.x0, lrow.x1)
                            y: y0 - 1
                            width: Math.abs(lrow.x1 - lrow.x0) - 4
                            height: 2
                            color: lrow.tint
                            Component.onCompleted: if (lrow.x1 < lrow.x0) x = Qt.binding(() => Math.min(lrow.x0, lrow.x1) + 4)
                        }
                        QQC2.Label {
                            text: lrow.x1 > lrow.x0 ? "▶" : "◀"
                            color: lrow.tint
                            font: Kirigami.Theme.smallFont
                            x: lrow.x1 > lrow.x0 ? lrow.x1 - width + 1 : lrow.x1 - 1
                            y: lrow.height * 0.68 - height / 2
                        }
                        QQC2.Label {
                            x: (lrow.x0 + lrow.x1) / 2 - width / 2
                            y: lrow.height * 0.68 - height - 1
                            text: ladder.label(lrow.modelData)
                            color: lrow.tint
                            font.weight: lrow.modelData.code ? Font.Normal : Font.DemiBold
                        }
                    }
                }
            }
        }
    }
}
