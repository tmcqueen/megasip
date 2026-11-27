import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts

// Agent Training: the board a connected agent is steering, drawn from the JSON the
// controller publishes (size, cells head first, food, score, alive).
Item {
    id: board
    property var model: ({})
    readonly property int cellsPerSide: model.size || 16
    readonly property real side: Math.max(0, Math.min(width, height - caption.height - Units.smallSpacing))

    ColumnLayout {
        anchors.centerIn: parent
        spacing: Units.smallSpacing

        RowLayout {
            id: caption
            Layout.preferredWidth: board.side
            QQC2.Label {
                text: "Agent Training"
                font: Theme.smallFont
                opacity: 0.7
            }
            Item { Layout.fillWidth: true }
            QQC2.Label {
                text: "Score " + (board.model.score || 0) + (board.model.best ? "  ·  Best " + board.model.best : "")
                font: Theme.smallFont
                opacity: 0.7
            }
        }

        Rectangle {
            Layout.preferredWidth: board.side
            Layout.preferredHeight: board.side
            radius: 4
            color: Theme.viewBackgroundColor
            border.color: Theme.separatorColor
            border.width: 1

            Canvas {
                id: canvas
                anchors.fill: parent
                anchors.margins: 2
                property var m: board.model
                onMChanged: requestPaint()
                onWidthChanged: requestPaint()
                onPaint: {
                    const ctx = getContext("2d")
                    ctx.reset()
                    const n = board.cellsPerSide
                    const c = width / n
                    const gap = Math.max(1, c * 0.12)
                    function cell(x, y, col, r) {
                        ctx.fillStyle = col
                        const s = c - gap
                        const px = x * c + gap / 2, py = y * c + gap / 2
                        ctx.beginPath()
                        ctx.roundedRect(px, py, s, s, r * s, r * s)
                        ctx.fill()
                    }
                    if (m.food) cell(m.food[0], m.food[1], Theme.neutralTextColor, 0.5)
                    const cells = m.cells || []
                    for (let i = cells.length - 1; i >= 0; i--) {
                        const col = i === 0 ? Theme.highlightColor
                                  : Qt.tint(Theme.highlightColor, Qt.rgba(Theme.viewBackgroundColor.r, Theme.viewBackgroundColor.g, Theme.viewBackgroundColor.b, 0.35))
                        cell(cells[i][0], cells[i][1], m.alive === false ? Theme.disabledTextColor : col, i === 0 ? 0.35 : 0.2)
                    }
                }
            }

            QQC2.Label {
                anchors.centerIn: parent
                visible: board.model.alive === false
                text: "Run over"
                font.pointSize: Theme.defaultFont.pointSize * 1.3
                padding: Units.smallSpacing
                background: Rectangle { color: Theme.backgroundColor; opacity: 0.85; radius: 4 }
            }
        }
    }
}
