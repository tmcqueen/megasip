import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts

// A small line graph of the last minute of one metric.
ColumnLayout {
    id: sp
    property string title
    property var values: []
    property int version: 0
    /// The vertical scale never shrinks below this (so small noise doesn't look dramatic).
    property real floor: 1
    property bool fixedMax: false
    spacing: 2

    readonly property real latest: { version; return values.length ? values[values.length - 1] : 0 }
    readonly property real scaleMax: {
        version
        if (fixedMax) return floor
        let m = floor
        for (const v of values) if (v > m) m = v
        return m
    }

    RowLayout {
        Layout.fillWidth: true
        QQC2.Label { text: sp.title; font: Theme.smallFont; opacity: 0.7; Layout.fillWidth: true }
        QQC2.Label { text: sp.latest.toFixed(sp.latest < 10 ? 1 : 0); font: Theme.smallFont }
    }
    Canvas {
        id: c
        Layout.fillWidth: true
        Layout.preferredHeight: Units.gridUnit * 3
        renderStrategy: Canvas.Immediate
        property int v: sp.version
        onVChanged: requestPaint()
        onAvailableChanged: requestPaint()
        onWidthChanged: requestPaint()
        onPaint: {
            const ctx = getContext("2d")
            ctx.reset()
            ctx.fillStyle = Qt.alpha(Theme.textColor, 0.05)
            ctx.fillRect(0, 0, width, height)
            const vals = sp.values
            if (vals.length < 2) return
            const n = 60
            const step = width / (n - 1)
            const x0 = width - (vals.length - 1) * step
            ctx.strokeStyle = Theme.highlightColor
            ctx.lineWidth = 1.5
            ctx.beginPath()
            for (let i = 0; i < vals.length; i++) {
                const x = x0 + i * step
                const y = height - 2 - (Math.min(vals[i], sp.scaleMax) / sp.scaleMax) * (height - 4)
                if (i === 0) ctx.moveTo(x, y); else ctx.lineTo(x, y)
            }
            ctx.stroke()
        }
    }
}
