pragma Singleton
import QtQuick

// Colours and fonts from the system palette (whatever style Qt is using), plus the
// state colours MegaSIP uses for good, warning and bad.
QtObject {
    readonly property SystemPalette _p: SystemPalette { colorGroup: SystemPalette.Active }
    readonly property SystemPalette _d: SystemPalette { colorGroup: SystemPalette.Disabled }
    readonly property bool dark: _p.window.hslLightness < 0.5

    readonly property color textColor: _p.windowText
    readonly property color disabledTextColor: Qt.rgba(_p.windowText.r, _p.windowText.g, _p.windowText.b, 0.6)
    readonly property color backgroundColor: _p.window
    readonly property color viewBackgroundColor: _p.base
    readonly property color highlightColor: _p.highlight
    readonly property color highlightedTextColor: _p.highlightedText
    readonly property color linkColor: dark ? "#3daee9" : "#2980b9"
    readonly property color visitedLinkColor: dark ? "#a370c9" : "#9b59b6"
    readonly property color negativeTextColor: "#da4453"
    readonly property color neutralTextColor: "#f67400"
    readonly property color positiveTextColor: "#27ae60"
    readonly property color negativeBackgroundColor: Qt.tint(_p.window, Qt.rgba(0.85, 0.27, 0.33, 0.2))
    readonly property color neutralBackgroundColor: Qt.tint(_p.window, Qt.rgba(0.96, 0.45, 0, 0.2))
    readonly property color positiveBackgroundColor: Qt.tint(_p.window, Qt.rgba(0.15, 0.68, 0.38, 0.2))
    readonly property color separatorColor: Qt.tint(_p.windowText, Qt.rgba(_p.window.r, _p.window.g, _p.window.b, 0.8))

    readonly property font defaultFont: Qt.application.font
    readonly property font smallFont: Qt.font({
        family: Qt.application.font.family,
        pointSize: Math.max(7, Math.round(Qt.application.font.pointSize * 0.8 * 10) / 10)
    })
    // Set by the C++ start-up from QFontDatabase's fixed-width system font.
    readonly property font fixedWidthFont: Qt.font({ family: typeof fixedFontFamily === "string" ? fixedFontFamily : "monospace" })
}
