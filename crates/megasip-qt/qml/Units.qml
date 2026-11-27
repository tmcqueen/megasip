pragma Singleton
import QtQuick

// Sizes and spacings, scaled to the system font like KDE's Kirigami.Units, so the
// layouts keep their proportions on any desktop and DPI.
QtObject {
    readonly property FontMetrics _fm: FontMetrics {}
    // One "grid unit" is the height of a line of default text, rounded to an even number.
    readonly property int gridUnit: { const h = Math.ceil(_fm.height); return h + (h % 2) }
    readonly property int smallSpacing: 4
    readonly property int mediumSpacing: 6
    readonly property int largeSpacing: 8
    readonly property int toolTipDelay: 700
    readonly property QtObject iconSizes: QtObject {
        readonly property int small: 16
        readonly property int smallMedium: 22
        readonly property int medium: 32
        readonly property int large: 48
        readonly property int huge: 64
    }
}
