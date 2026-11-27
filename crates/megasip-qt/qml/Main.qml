import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import Qt.labs.platform as Platform
import org.megasip

QQC2.ApplicationWindow {
    id: root
    title: activeAcc ? "MegaSIP — " + accTitle(activeAcc) : "MegaSIP"
    width: Units.gridUnit * 21
    height: Units.gridUnit * 36
    minimumWidth: Units.gridUnit * 17
    minimumHeight: Units.gridUnit * 24
    visible: !startHidden
    flags: settings.alwaysOnTop ? (Qt.Window | Qt.WindowStaysOnTopHint) : Qt.Window

    property var accounts: phone.accountsJson ? JSON.parse(phone.accountsJson) : []
    property var calls: phone.callsJson ? JSON.parse(phone.callsJson) : []
    property var history: phone.historyJson ? JSON.parse(phone.historyJson) : []
    property var contacts: phone.contactsJson ? JSON.parse(phone.contactsJson) : []
    property var settings: phone.settingsJson ? JSON.parse(phone.settingsJson) : ({})
    property var devices: phone.devicesJson ? JSON.parse(phone.devicesJson) : ({ outputs: [], inputs: [] })
    property int currentTab: 0

    // One account per window: this window's, the ones it can pick, and the ones other
    // windows have active.
    function accTitle(a) {
        if (a.label && a.label.trim()) return a.label.trim()
        if (a.username) return a.username + "@" + ((a.domain && a.domain.trim()) || (a.server || "").trim())
        return "New account"
    }
    property var activeAcc: (settings.accounts || []).find(a => a.id === phone.activeAccount) || null
    property var pickable: (settings.accounts || []).filter(a => a.enabled && a.server && a.server.trim())
    property var inUse: []
    function refreshInUse() {
        const v = JSON.parse(phone.accountsInUse() || "[]")
        if (JSON.stringify(v) !== JSON.stringify(inUse)) inUse = v
    }
    Timer {
        interval: 3000
        repeat: true
        running: true
        triggeredOnStart: true
        onTriggered: root.refreshInUse()
    }
    property var selectedCall: {
        // The call the dialpad and buttons act on: the active (not held) one first.
        for (let c of calls) if (c.state === "connected" && !c.held) return c
        for (let c of calls) if (c.state !== "incoming") return c
        return calls.length ? calls[0] : null
    }
    property var incomingCalls: calls.filter(c => c.state === "incoming")
    property string pendingTransferMode: ""

    function show() {
        root.visible = true
        root.raise()
        root.requestActivate()
    }
    function toggle() {
        if (root.visible && root.active) root.hide(); else root.show()
    }
    function dial(number) {
        if (!number || !number.trim()) return
        phone.dial(number, phone.activeAccount)
        currentTab = 0
    }
    function displayName(c) {
        return c.name ? c.name : c.number
    }
    function formatDuration(s) {
        s = Math.max(0, Math.floor(s))
        const h = Math.floor(s / 3600), m = Math.floor((s % 3600) / 60), sec = s % 60
        const mm = (h ? String(m).padStart(2, "0") : String(m)), ss = String(sec).padStart(2, "0")
        return h ? `${h}:${mm}:${ss}` : `${mm}:${ss}`
    }
    property real now: Date.now() / 1000
    Timer { interval: 1000; running: root.calls.length > 0; repeat: true; onTriggered: root.now = Date.now() / 1000 }

    Phone {
        id: phone
        onIncomingCall: (id, name, number) => {
            incomingWindow.showFor(id)
            notifier.incoming(id, name || number, name ? number : qsTr("Incoming call"))
        }
        onMissedCall: (name, number) => notifier.missed(qsTr("Missed call"), name ? `${name} (${number})` : number)
        onErrorMessage: text => {
            errorMessage.text = text
            errorMessage.visible = true
            errorTimer.restart()
        }
        onActivateRequested: root.show()
    }


    Notifier {
        id: notifier
        tray: tray
        onAnswerRequested: id => phone.answer(id)
        onDeclineRequested: id => phone.reject(id)
        onMissedClicked: { root.show(); root.currentTab = 2 }
        onIncomingClicked: root.show()
    }
    Connections {
        target: root
        function onIncomingCallsChanged() {
            if (root.incomingCalls.length === 0) {
                notifier.closeIncoming()
                incomingWindow.close()
            }
        }
    }

    // The active account's own icon, if it has one: faded while it isn't registered.
    readonly property bool registered: root.accounts.some(a => a.state === "registered")
    readonly property string accountIcon: root.activeAcc && root.activeAcc.icon
        ? phone.accountIcon(root.activeAcc.icon, root.activeAcc.iconColor || "", root.accTitle(root.activeAcc),
                            !root.registered && root.calls.length === 0)
        : ""
    Platform.SystemTrayIcon {
        id: tray
        visible: true
        // A missed call still shows as one; otherwise the account's icon wins.
        icon.source: phone.missedCount > 0 ? "" : root.accountIcon
        icon.name: phone.missedCount > 0 ? "call-missed"
                 : root.accountIcon ? ""
                 : root.calls.length > 0 ? "call-start"
                 : root.registered ? "call-start-symbolic" : "call-stop"
        tooltip: "MegaSIP — " + (root.activeAcc ? root.accTitle(root.activeAcc) + ": " : "") + phone.statusText + (phone.sourceLabel ? " · playing " + phone.sourceLabel : "")
        onActivated: reason => {
            if (reason === Platform.SystemTrayIcon.Trigger) root.toggle()
        }
        menu: Platform.Menu {
            id: trayMenu
            onAboutToShow: root.refreshInUse()
            Platform.MenuItem { text: root.visible ? "Hide" : "Show"; onTriggered: root.toggle() }
            Platform.MenuSeparator {}
            Platform.MenuItem { text: "Accounts"; enabled: false; visible: root.pickable.length > 0 }
            // Account items are inserted here (index 3 on) by the Instantiator below.
            Platform.MenuItem {
                text: root.activeAcc ? "Deactivate " + root.accTitle(root.activeAcc) : ""
                visible: !!root.activeAcc
                onTriggered: phone.deactivateAccount()
            }
            Platform.MenuSeparator {}
            Platform.MenuItem { text: "Do Not Disturb"; checkable: true; checked: phone.dnd; onTriggered: phone.setDndOn(!phone.dnd) }
            Platform.MenuItem { text: "Auto Answer"; checkable: true; checked: phone.autoAnswer; onTriggered: phone.setAutoAnswerOn(!phone.autoAnswer) }
            Platform.MenuItem { text: "Redial"; enabled: !!root.settings.lastNumber; onTriggered: phone.redial() }
            Platform.MenuSeparator {}
            Platform.MenuItem { text: "Preferences…"; onTriggered: prefs.openPage() }
            Platform.MenuItem { text: "Debug window…"; onTriggered: debugWin.openTab() }
            Platform.MenuItem { text: "Quit"; onTriggered: phone.quit() }
        }
    }

    Instantiator {
        model: root.pickable
        delegate: Platform.MenuItem {
            required property var modelData
            text: root.accTitle(modelData) + (root.inUse.includes(modelData.id) ? "  (in another window)" : "")
            checkable: true
            checked: modelData.id === phone.activeAccount
            enabled: !root.inUse.includes(modelData.id)
            onTriggered: {
                phone.pickAccount(modelData.id)
                checked = Qt.binding(() => modelData.id === phone.activeAccount)
            }
        }
        onObjectAdded: (index, object) => trayMenu.insertItem(3 + index, object)
        onObjectRemoved: (index, object) => trayMenu.removeItem(object)
    }

    onClosing: close => {
        // Like MicroSIP: closing the window keeps the phone running in the tray.
        close.accepted = false
        root.hide()
    }

    PreferencesWindow {
        id: prefs
        phone: phone
        devices: root.devices
        status: root.accounts
        transientParent: root
        onDebugRequested: debugWin.openTab(0)
    }
    DebugWindow {
        id: debugWin
        phone: phone
    }
    // First run: nothing to call with yet, so go straight to adding an account.
    Timer {
        interval: 600
        running: true
        onTriggered: if (root.visible && (!root.settings.accounts || root.settings.accounts.length === 0)) prefs.openPage(0)
    }
    // Debug aid for MEGASIP_SCREENSHOT: steps 0-2 are tabs, then each preferences page.
    property int shotStep: -1
    onShotStepChanged: {
        if (shotStep < 3) { prefs.hide(); root.currentTab = shotStep }
        else if (shotStep < 10) prefs.openPage(shotStep - 3)
        else if (shotStep < 14) { prefs.forceClose = true; prefs.close(); debugWin.openTab(shotStep - 10) }
        // 14: a message in the trace pane; 15: after Clear (trace); 16: after Clear (quality)
        else if (shotStep === 14) { debugWin.openTab(0); debugWin.showMessage("MESSAGE SHOWN BEFORE CLEAR") }
        else if (shotStep === 15) { debugWin.clearAll(); debugWin.openTab(0) }
        else if (shotStep === 16) debugWin.openTab(2)
        // 17: the contacts list and the contact editor; 18: an error banner and the transfer dialog
        else if (shotStep === 17) { debugWin.close(); root.currentTab = 1; contactsPage.editNew("Ada Lovelace", "+15551234567") }
        else if (shotStep === 18) { contactsPage.closeEditor(); errorMessage.text = "Couldn't reach the server"; errorMessage.visible = true; root.currentTab = 0; transferDialog.openFor(0) }
        // 19: the "Save your changes?" prompt
        else { transferDialog.close(); prefs.openPage(1); prefs.dirty = true; prefs.forceClose = false; prefs.close() }
    }

    IncomingWindow {
        id: incomingWindow
        phone: phone
        calls: root.incomingCalls
        accounts: root.accounts
    }

    TransferDialog {
        id: transferDialog
        calls: root.calls
        contacts: root.contacts
        onBlind: (id, target) => phone.transfer(id, target)
        onAttended: (id, other) => phone.attendedTransfer(id, other)
    }

    Timer { id: errorTimer; interval: 8000; onTriggered: errorMessage.visible = false }

    ColumnLayout {
        anchors.fill: parent
        spacing: 0

        StatusBar {
            Layout.fillWidth: true
            phone: phone
            accounts: root.accounts
            settings: root.settings
            activeAcc: root.activeAcc
            pickable: root.pickable
            inUse: root.inUse
            accTitle: root.accTitle
            onMenuOpening: root.refreshInUse()
            onPreferencesRequested: prefs.openPage()
            onAccountRequested: prefs.openPage(0)
        }

        InlineMessage {
            id: errorMessage
            Layout.fillWidth: true
            Layout.margins: Units.smallSpacing
            type: "error"
            showCloseButton: true
            visible: false
        }

        StackLayout {
            Layout.fillWidth: true
            Layout.fillHeight: true
            currentIndex: root.currentTab

            DialpadPage {
                phone: phone
                activeCall: root.selectedCall
                settings: root.settings
                contacts: root.contacts
                history: root.history
                onDialRequested: number => root.dial(number)
            }
            ContactsPage {
                id: contactsPage
                phone: phone
                contacts: root.contacts
                onDialRequested: number => root.dial(number)
            }
            HistoryPage {
                phone: phone
                history: root.history
                accounts: root.accounts
                onDialRequested: number => root.dial(number)
                onAddContact: (name, number) => { root.currentTab = 1; contactsPage.editNew(name, number) }
                visible: StackLayout.isCurrentItem
                onVisibleChanged: if (visible) phone.seenMissed()
            }
        }

        CallsPanel {
            Layout.fillWidth: true
            phone: phone
            calls: root.calls
            now: root.now
            selected: root.selectedCall
            visible: root.calls.length > 0
            onTransferRequested: id => transferDialog.openFor(id)
            onQualityRequested: debugWin.openTab(2)
            formatDuration: root.formatDuration
        }
    }


    footer: NavigationTabBar {
        actions: [
            QQC2.Action {
                icon.name: "input-dialpad-symbolic"
                text: "Dialpad"
                checked: root.currentTab === 0
                onTriggered: root.currentTab = 0
            },
            QQC2.Action {
                icon.name: "im-user"
                text: "Contacts"
                checked: root.currentTab === 1
                onTriggered: root.currentTab = 1
            },
            QQC2.Action {
                icon.name: phone.missedCount > 0 ? "call-missed" : "view-history"
                text: phone.missedCount > 0 ? `Calls (${phone.missedCount})` : "Calls"
                checked: root.currentTab === 2
                onTriggered: root.currentTab = 2
            }
        ]
    }

    Shortcut { sequence: "Ctrl+Q"; onActivated: phone.quit() }
    Shortcut { sequence: "Ctrl+1"; onActivated: root.currentTab = 0 }
    Shortcut { sequence: "Ctrl+2"; onActivated: root.currentTab = 1 }
    Shortcut { sequence: "Ctrl+3"; onActivated: root.currentTab = 2 }
    Shortcut { sequence: "Ctrl+,"; onActivated: prefs.openPage() }
    Shortcut { sequence: "Ctrl+Shift+D"; onActivated: debugWin.openTab() }
    Shortcut {
        sequence: "Escape"
        enabled: root.selectedCall !== null && root.currentTab === 0
        onActivated: phone.hangup(root.selectedCall.id)
    }
}
