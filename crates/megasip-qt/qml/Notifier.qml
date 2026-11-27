import QtQuick

// Desktop notifications for incoming and missed calls. On Plasma (or anywhere KDE's
// notification QML module is installed) they're KNotifications, with Answer / Decline on
// the incoming one. Elsewhere, Windows included, they're tray balloons; the incoming-call
// window has the buttons there.
QtObject {
    id: n
    // The Platform.SystemTrayIcon used for the fallback.
    required property var tray
    signal answerRequested(int callId)
    signal declineRequested(int callId)
    signal missedClicked()
    signal incomingClicked()

    // Built from a string so that a missing org.kde.notification module is an error we can
    // catch here, not one that stops the whole UI from loading.
    property var kde: null
    property string lastTrayKind: ""
    Component.onCompleted: {
        try {
            kde = Qt.createQmlObject(`
                import QtQuick
                import org.kde.notification
                QtObject {
                    id: k
                    signal answer(int callId)
                    signal decline(int callId)
                    signal missedActivated()
                    property Notification ring: Notification {
                        property int callId: 0
                        componentName: "megasip"
                        eventId: "incomingCall"
                        urgency: Notification.CriticalUrgency
                        flags: Notification.Persistent
                        actions: [
                            NotificationAction { label: "Answer"; onActivated: k.answer(k.ring.callId) },
                            NotificationAction { label: "Decline"; onActivated: k.decline(k.ring.callId) }
                        ]
                    }
                    property Component missedComponent: Component {
                        Notification {
                            componentName: "megasip"
                            eventId: "missedCall"
                            iconName: "call-missed"
                            autoDelete: true
                            defaultAction: NotificationAction { onActivated: k.missedActivated() }
                        }
                    }
                    function incoming(callId, title, text) {
                        ring.callId = callId; ring.title = title; ring.text = text; ring.sendEvent()
                    }
                    function missed(title, text) {
                        missedComponent.createObject(k, { title: title, text: text }).sendEvent()
                    }
                    function closeIncoming() { ring.close() }
                }`, n, "KdeNotifier")
            kde.answer.connect(id => n.answerRequested(id))
            kde.decline.connect(id => n.declineRequested(id))
            kde.missedActivated.connect(() => n.missedClicked())
        } catch (e) {
            kde = null
        }
    }

    function incoming(callId, title, text) {
        if (kde) { kde.incoming(callId, title, text); return }
        lastTrayKind = "incoming"
        tray.showMessage(title, text, 0, 30000)
    }
    function missed(title, text) {
        if (kde) { kde.missed(title, text); return }
        lastTrayKind = "missed"
        tray.showMessage(title, text, 0, 10000)
    }
    function closeIncoming() {
        if (kde) kde.closeIncoming()
    }
    property Connections _tray: Connections {
        target: n.tray
        function onMessageClicked() {
            if (n.lastTrayKind === "missed") n.missedClicked(); else n.incomingClicked()
        }
    }
}
