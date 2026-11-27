import QtQuick
import QtQuick.Effects

// A themed icon by name (from the system icon theme, or MegaSIP's bundled icons where
// there is none), or an image by URL. isMask tints a symbolic icon with `color`.
Item {
    id: icon
    property string source: ""
    property color color: Theme.textColor
    property bool isMask: false
    implicitWidth: Units.iconSizes.smallMedium
    implicitHeight: implicitWidth

    readonly property bool isUrl: source.indexOf(":") > 0 || source.startsWith("/")
    Image {
        id: img
        anchors.fill: parent
        visible: !icon.isMask
        source: !icon.source ? "" : icon.isUrl ? icon.source : "image://icon/" + icon.source
        sourceSize.width: Math.ceil(icon.width * Screen.devicePixelRatio)
        sourceSize.height: Math.ceil(icon.height * Screen.devicePixelRatio)
        fillMode: Image.PreserveAspectFit
        smooth: true
        asynchronous: false
    }
    MultiEffect {
        anchors.fill: img
        source: img
        visible: icon.isMask
        colorization: 1.0
        colorizationColor: icon.color
        brightness: 1.0
    }
}
