import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

// Codecs: tick to enable, arrows to set priority (top = preferred).
ColumnLayout {
    id: cl
    property var enabledCodecs: []
    signal codecsChanged(var list)
    spacing: 0

    readonly property var names: ({ opus: "Opus — wideband, best quality", g722: "G.722 — HD voice", pcmu: "G.711 µ-law", pcma: "G.711 A-law" })
    property var order: {
        const all = ["opus", "g722", "pcmu", "pcma"]
        return enabledCodecs.concat(all.filter(c => !enabledCodecs.includes(c)))
    }

    function move(from, to) {
        const o = cl.order.slice()
        const t = o[to]; o[to] = o[from]; o[from] = t
        const en = o.filter(c => cl.enabledCodecs.includes(c))
        cl.enabledCodecs = en
        cl.codecsChanged(en)
    }

    Repeater {
        model: cl.order
        delegate: RowLayout {
            id: row
            required property var modelData
            required property int index
            Layout.fillWidth: true
            QQC2.CheckBox {
                Layout.fillWidth: true
                text: cl.names[row.modelData]
                checked: cl.enabledCodecs.includes(row.modelData)
                onToggled: {
                    let en = cl.enabledCodecs.slice()
                    if (checked) en.push(row.modelData); else en = en.filter(c => c !== row.modelData)
                    if (en.length === 0) { checked = true; return }   // keep at least one
                    en = cl.order.filter(c => en.includes(c))
                    cl.enabledCodecs = en
                    cl.codecsChanged(en)
                }
            }
            QQC2.ToolButton {
                icon.name: "go-up"
                enabled: row.index > 0
                onClicked: cl.move(row.index, row.index - 1)
                QQC2.ToolTip.text: "Prefer"
                QQC2.ToolTip.visible: hovered
            }
            QQC2.ToolButton {
                icon.name: "go-down"
                enabled: row.index < cl.order.length - 1
                onClicked: cl.move(row.index, row.index + 1)
                QQC2.ToolTip.text: "Prefer less"
                QQC2.ToolTip.visible: hovered
            }
        }
    }
}
