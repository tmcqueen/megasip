import QtQuick
import QtQuick.Controls as QQC2

// A round badge with someone's initials, coloured by their name.
Rectangle {
    id: av
    property string name: ""
    readonly property string initials: {
        const words = name.trim().split(/\s+/).filter(w => w.length > 0)
        if (words.length === 0) return ""
        // A number gets its first digit.
        if (/^[+\d]/.test(words[0])) return words[0].replace(/^\+/, "").charAt(0)
        return (words[0].charAt(0) + (words.length > 1 ? words[words.length - 1].charAt(0) : "")).toUpperCase()
    }
    readonly property var colors: ["#e93a9a", "#e93d58", "#e9643a", "#ef973c", "#e8cb2d", "#b6e521",
                                    "#3dd425", "#00d485", "#00d3b8", "#3daee9", "#b875dc", "#926ee4", "#686b9d"]
    // FNV-1a, so similar names still get different colours.
    function hash(s) { let h = 0x811c9dc5; for (let i = 0; i < s.length; i++) h = Math.imul(h ^ s.charCodeAt(i), 0x01000193); return h >>> 0 }
    implicitWidth: Units.iconSizes.medium
    implicitHeight: implicitWidth
    radius: width / 2
    color: colors[hash(name) % colors.length]
    QQC2.Label {
        anchors.centerIn: parent
        text: av.initials
        color: "white"
        font.pixelSize: av.height * 0.42
        font.bold: true
    }
}
