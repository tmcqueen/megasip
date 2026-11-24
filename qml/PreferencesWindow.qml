import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import QtQuick.Dialogs as Dialogs
import org.kde.kirigami as Kirigami

// Preferences: a modal window with categories on the left and the chosen page on the
// right. Changes collect in a draft and are applied with Save (accounts re-register then,
// not on every keystroke).
QQC2.ApplicationWindow {
    id: win
    required property var phone
    property var devices: ({ outputs: [], inputs: [] })
    property var status: []

    title: "Preferences"
    flags: Qt.Dialog
    modality: Qt.ApplicationModal
    width: Kirigami.Units.gridUnit * 46
    height: Kirigami.Units.gridUnit * 38
    minimumWidth: Kirigami.Units.gridUnit * 34
    minimumHeight: Kirigami.Units.gridUnit * 24
    visible: false
    color: Kirigami.Theme.backgroundColor

    property var draft: ({})
    property bool dirty: false
    property int version: 0
    property string saveError: ""
    property int current: 0
    property int selAcc: 0
    property bool showAdvancedAccount: false
    property bool forceClose: false

    readonly property var logLevels: ["off", "error", "warn", "info", "debug", "trace"]
    readonly property var logLevelNames: ["Off", "Errors", "Warnings", "Info", "Debug", "Trace"]
    readonly property var pages: [
        { name: "Accounts", icon: "user-identity", component: accountsPage },
        { name: "Audio", icon: "audio-volume-high", component: audioPage },
        { name: "Calls", icon: "call-start", component: callsPage },
        { name: "Availability", icon: "mail-forward", component: availabilityPage },
        { name: "General", icon: "preferences-system", component: generalPage },
        { name: "Network", icon: "network-wired", component: networkPage },
        { name: "Logging", icon: "view-list-details", component: loggingPage }
    ]

    function openPage(i) {
        if (!visible || !dirty) reload()
        current = i === undefined ? current : i
        phone.refreshDevices()
        show()
        raise()
        requestActivate()
    }
    function reload() {
        draft = JSON.parse(phone.settingsJson || "{}")
        if (!draft.accounts) draft.accounts = []
        const d = draft.accounts.findIndex(a => a.id === draft.defaultAccount)
        if (selAcc < 0 || selAcc >= draft.accounts.length) selAcc = Math.max(0, d)
        dirty = false
        saveError = ""
        version++
    }
    function set(key, value) {
        if (draft[key] === value) return
        draft[key] = value
        dirty = true
    }
    function acc() { return draft.accounts && draft.accounts[selAcc] ? draft.accounts[selAcc] : null }
    function setAcc(key, value) {
        const a = acc()
        if (!a || a[key] === value) return
        a[key] = value
        dirty = true
    }
    function save() {
        for (let a of draft.accounts) {
            a.server = (a.server || "").trim()
            a.username = (a.username || "").trim()
        }
        const err = phone.saveSettings(JSON.stringify(draft))
        if (err) { saveError = err; return false }
        dirty = false
        reload()
        return true
    }
    function statusOf(id) {
        for (let s of status) if (s.id === id) return s
        return null
    }
    function stateColor(s) {
        if (s === "registered") return Kirigami.Theme.positiveTextColor
        if (s === "error") return Kirigami.Theme.negativeTextColor
        if (s === "registering") return Kirigami.Theme.neutralTextColor
        return Kirigami.Theme.disabledTextColor
    }
    function accTitle(a) {
        if (!a) return ""
        if (a.label) return a.label
        if (a.username && a.server) return a.username + "@" + (a.domain || a.server)
        return "New account"
    }
    function devIndex(list, name) {
        const i = list.indexOf(name)
        return i < 0 ? 0 : i + 1
    }

    Connections {
        target: win.phone
        function onSettingsJsonChanged() { if (win.visible && !win.dirty) win.reload() }
    }

    onClosing: close => {
        if (dirty && !forceClose) {
            close.accepted = false
            unsaved.open()
        }
        forceClose = false
    }

    Kirigami.PromptDialog {
        id: unsaved
        title: "Save your changes?"
        subtitle: "You changed some preferences without saving them."
        standardButtons: Kirigami.Dialog.Save | Kirigami.Dialog.Discard | Kirigami.Dialog.Cancel
        onAccepted: if (win.save()) win.close()
        onDiscarded: { win.reload(); win.forceClose = true; win.close(); unsaved.close() }
    }

    Shortcut { sequence: "Escape"; onActivated: win.close() }
    Shortcut { sequences: [StandardKey.Save]; onActivated: win.save() }

    RowLayout {
        anchors.fill: parent
        spacing: 0

        // ------------------------------------------------------------ sidebar
        QQC2.Pane {
            Layout.fillHeight: true
            Layout.preferredWidth: Kirigami.Units.gridUnit * 13
            padding: Kirigami.Units.largeSpacing
            Kirigami.Theme.colorSet: Kirigami.Theme.Window
            Kirigami.Theme.inherit: false
            background: Rectangle { color: Kirigami.Theme.backgroundColor }
            contentItem: ListView {
                id: sidebar
                model: win.pages
                currentIndex: win.current
                spacing: 2
                interactive: false
                delegate: QQC2.ItemDelegate {
                    required property var modelData
                    required property int index
                    width: ListView.view.width
                    text: modelData.name
                    icon.name: modelData.icon
                    highlighted: index === win.current
                    onClicked: win.current = index
                }
            }
        }
        Kirigami.Separator { Layout.fillHeight: true }

        // ------------------------------------------------------------ content
        ColumnLayout {
            Layout.fillWidth: true
            Layout.fillHeight: true
            spacing: 0

            QQC2.ScrollView {
                id: scroller
                Layout.fillWidth: true
                Layout.fillHeight: true
                contentWidth: availableWidth
                clip: true

                ColumnLayout {
                    width: scroller.availableWidth
                    spacing: 0
                    Kirigami.Heading {
                        Layout.fillWidth: true
                        Layout.margins: Kirigami.Units.gridUnit
                        Layout.bottomMargin: Kirigami.Units.largeSpacing
                        level: 2
                        text: win.pages[win.current].name
                    }
                    Loader {
                        id: pageLoader
                        Layout.fillWidth: true
                        Layout.leftMargin: Kirigami.Units.gridUnit
                        Layout.rightMargin: Kirigami.Units.gridUnit
                        Layout.bottomMargin: Kirigami.Units.gridUnit
                        // Rebuilt on page change and on reload/revert, so every field shows the draft.
                        property string key: win.current + ":" + win.version
                        sourceComponent: win.pages[win.current].component
                        onKeyChanged: { active = false; active = true }
                    }
                }
            }

            QQC2.ToolBar {
                Layout.fillWidth: true
                position: QQC2.ToolBar.Footer
                contentItem: RowLayout {
                    QQC2.Label {
                        Layout.fillWidth: true
                        text: win.saveError || (win.dirty ? "You have unsaved changes" : "")
                        color: win.saveError ? Kirigami.Theme.negativeTextColor : Kirigami.Theme.textColor
                        elide: Text.ElideRight
                    }
                    QQC2.Button {
                        text: "Revert"
                        icon.name: "edit-undo"
                        enabled: win.dirty
                        onClicked: win.reload()
                    }
                    QQC2.Button {
                        text: "Save"
                        icon.name: "dialog-ok-apply"
                        highlighted: true
                        enabled: win.dirty
                        onClicked: win.save()
                    }
                    QQC2.Button {
                        text: "Close"
                        onClicked: win.close()
                    }
                }
            }
        }
    }

    // ================================================================= Accounts
    Component {
        id: accountsPage
        ColumnLayout {
            spacing: 0
            PrefSection {
                title: "Your accounts"
                description: "Calls go out on the default account. Incoming calls ring on all of them."
                Repeater {
                    model: win.draft.accounts ? win.draft.accounts.length : 0
                    delegate: QQC2.ItemDelegate {
                        id: accRow
                        required property int index
                        property var a: win.draft.accounts[index]
                        property var st: win.statusOf(a.id)
                        Layout.fillWidth: true
                        highlighted: index === win.selAcc
                        onClicked: { win.selAcc = index; win.version++ }
                        contentItem: RowLayout {
                            Rectangle {
                                implicitWidth: Kirigami.Units.gridUnit * 0.6
                                implicitHeight: implicitWidth
                                radius: width / 2
                                color: win.stateColor(accRow.st ? accRow.st.state : "off")
                            }
                            ColumnLayout {
                                Layout.fillWidth: true
                                spacing: 0
                                QQC2.Label { text: win.accTitle(accRow.a); font.weight: Font.DemiBold; elide: Text.ElideRight; Layout.fillWidth: true }
                                QQC2.Label {
                                    Layout.fillWidth: true
                                    text: accRow.st ? accRow.st.text : (accRow.a.enabled === false ? "Disabled" : "Not saved yet")
                                    font: Kirigami.Theme.smallFont
                                    opacity: 0.75
                                    elide: Text.ElideRight
                                }
                            }
                            QQC2.Label {
                                text: "Default"
                                visible: accRow.a.id === win.draft.defaultAccount
                                font: Kirigami.Theme.smallFont
                                color: Kirigami.Theme.positiveTextColor
                            }
                        }
                    }
                }
                QQC2.Button {
                    text: "Add account"
                    icon.name: "list-add"
                    onClicked: {
                        const a = JSON.parse(win.phone.newAccountJson())
                        win.draft.accounts.push(a)
                        if (win.draft.accounts.length === 1) win.draft.defaultAccount = a.id
                        win.selAcc = win.draft.accounts.length - 1
                        win.dirty = true
                        win.version++
                    }
                }
            }

            ColumnLayout {
                Layout.fillWidth: true
                visible: win.acc() !== null
                spacing: 0

                PrefSection {
                    title: win.accTitle(win.acc())
                    description: "What your provider or PBX gave you."
                    GridLayout {
                        Layout.fillWidth: true
                        columns: width > Kirigami.Units.gridUnit * 24 ? 2 : 1
                        columnSpacing: Kirigami.Units.largeSpacing
                        rowSpacing: Kirigami.Units.largeSpacing
                        PrefField {
                            label: "SIP server"
                            QQC2.TextField {
                                Layout.fillWidth: true
                                placeholderText: "sip.example.com"
                                text: win.acc() ? win.acc().server || "" : ""
                                onTextEdited: win.setAcc("server", text.trim())
                            }
                        }
                        PrefField {
                            label: "Username"
                            QQC2.TextField {
                                Layout.fillWidth: true
                                placeholderText: "1001"
                                text: win.acc() ? win.acc().username || "" : ""
                                onTextEdited: win.setAcc("username", text.trim())
                            }
                        }
                        PrefField {
                            label: "Password"
                            Kirigami.PasswordField {
                                Layout.fillWidth: true
                                text: win.acc() ? win.acc().password || "" : ""
                                onTextEdited: win.setAcc("password", text)
                            }
                        }
                        PrefField {
                            label: "Display name"
                            hint: "Shown to the people you call"
                            QQC2.TextField {
                                Layout.fillWidth: true
                                text: win.acc() ? win.acc().displayName || "" : ""
                                onTextEdited: win.setAcc("displayName", text)
                            }
                        }
                        PrefField {
                            label: "Account name"
                            hint: "Optional, e.g. Work"
                            QQC2.TextField {
                                Layout.fillWidth: true
                                text: win.acc() ? win.acc().label || "" : ""
                                onTextEdited: win.setAcc("label", text)
                            }
                        }
                    }
                    QQC2.CheckBox {
                        text: "Enabled"
                        checked: win.acc() ? win.acc().enabled !== false : true
                        onToggled: win.setAcc("enabled", checked)
                    }
                    RowLayout {
                        QQC2.Button {
                            text: win.acc() && win.acc().id === win.draft.defaultAccount ? "Default for calls" : "Make default"
                            icon.name: "favorite"
                            enabled: win.acc() && win.acc().id !== win.draft.defaultAccount
                            onClicked: { win.set("defaultAccount", win.acc().id); win.version++ }
                        }
                        QQC2.Button {
                            text: "Delete account"
                            icon.name: "edit-delete-remove"
                            onClicked: confirmDelete.open()
                        }
                    }
                }

                PrefSection {
                    title: "Advanced"
                    description: "Only if your provider asks for it."
                    last: true
                    QQC2.Button {
                        text: win.showAdvancedAccount ? "Hide advanced settings" : "Show advanced settings"
                        icon.name: win.showAdvancedAccount ? "go-up" : "go-down"
                        flat: true
                        onClicked: win.showAdvancedAccount = !win.showAdvancedAccount
                    }
                    GridLayout {
                        Layout.fillWidth: true
                        visible: win.showAdvancedAccount
                        columns: width > Kirigami.Units.gridUnit * 24 ? 2 : 1
                        columnSpacing: Kirigami.Units.largeSpacing
                        rowSpacing: Kirigami.Units.largeSpacing
                        PrefField {
                            label: "Domain"
                            hint: "Leave empty to use the server"
                            QQC2.TextField { Layout.fillWidth: true; text: win.acc() ? win.acc().domain || "" : ""; onTextEdited: win.setAcc("domain", text.trim()) }
                        }
                        PrefField {
                            label: "Login (auth ID)"
                            hint: "Leave empty to use the username"
                            QQC2.TextField { Layout.fillWidth: true; text: win.acc() ? win.acc().authId || "" : ""; onTextEdited: win.setAcc("authId", text.trim()) }
                        }
                        PrefField {
                            label: "Outbound proxy"
                            QQC2.TextField { Layout.fillWidth: true; placeholderText: "None"; text: win.acc() ? win.acc().proxy || "" : ""; onTextEdited: win.setAcc("proxy", text.trim()) }
                        }
                        PrefField {
                            label: "Public address"
                            hint: "Leave empty to detect it"
                            QQC2.TextField { Layout.fillWidth: true; text: win.acc() ? win.acc().publicAddr || "" : ""; onTextEdited: win.setAcc("publicAddr", text.trim()) }
                        }
                        PrefField {
                            label: "Transport"
                            QQC2.ComboBox {
                                Layout.fillWidth: true
                                model: ["Auto (DNS)", "UDP", "TCP", "TLS"]
                                currentIndex: Math.max(0, ["auto", "udp", "tcp", "tls"].indexOf(win.acc() ? win.acc().transport || "auto" : "auto"))
                                onActivated: i => win.setAcc("transport", ["auto", "udp", "tcp", "tls"][i])
                            }
                        }
                        PrefField {
                            label: "Media encryption (SRTP)"
                            QQC2.ComboBox {
                                Layout.fillWidth: true
                                model: ["Off", "Optional", "Mandatory"]
                                currentIndex: Math.max(0, ["off", "optional", "mandatory"].indexOf(win.acc() ? win.acc().srtp || "off" : "off"))
                                onActivated: i => win.setAcc("srtp", ["off", "optional", "mandatory"][i])
                            }
                        }
                        PrefField {
                            label: "Register every"
                            QQC2.SpinBox { from: 30; to: 86400; editable: true; value: win.acc() ? win.acc().registerRefresh || 300 : 300; onValueModified: win.setAcc("registerRefresh", value) }
                            QQC2.Label { text: "seconds" }
                        }
                        PrefField {
                            label: "Keep-alive every"
                            hint: "0 turns it off"
                            QQC2.SpinBox { from: 0; to: 3600; editable: true; value: win.acc() ? (win.acc().keepAlive ?? 15) : 15; onValueModified: win.setAcc("keepAlive", value) }
                            QQC2.Label { text: "seconds" }
                        }
                        PrefField {
                            label: "Voicemail number"
                            QQC2.TextField { Layout.fillWidth: true; text: win.acc() ? win.acc().voicemailNumber || "" : ""; onTextEdited: win.setAcc("voicemailNumber", text.trim()) }
                        }
                        PrefField {
                            label: "Dialing prefix"
                            hint: "Added in front of numbers you dial"
                            QQC2.TextField { Layout.fillWidth: true; text: win.acc() ? win.acc().dialingPrefix || "" : ""; onTextEdited: win.setAcc("dialingPrefix", text.trim()) }
                        }
                    }
                    ColumnLayout {
                        visible: win.showAdvancedAccount
                        spacing: 0
                        QQC2.CheckBox {
                            text: "Hide my caller ID"
                            checked: win.acc() ? !!win.acc().hideCid : false
                            onToggled: win.setAcc("hideCid", checked)
                        }
                        QQC2.CheckBox {
                            text: "Re-register with the address the server sees (NAT)"
                            checked: win.acc() ? win.acc().allowRewrite !== false : true
                            onToggled: win.setAcc("allowRewrite", checked)
                        }
                    }
                }
            }

            Kirigami.PlaceholderMessage {
                Layout.fillWidth: true
                Layout.topMargin: Kirigami.Units.gridUnit
                visible: !win.draft.accounts || win.draft.accounts.length === 0
                icon.name: "user-identity"
                text: "No accounts yet"
                explanation: "Add the SIP account your provider or PBX gave you."
            }

            Kirigami.PromptDialog {
                id: confirmDelete
                title: "Delete " + win.accTitle(win.acc()) + "?"
                subtitle: "It's removed when you save."
                standardButtons: Kirigami.Dialog.Ok | Kirigami.Dialog.Cancel
                onAccepted: {
                    win.draft.accounts.splice(win.selAcc, 1)
                    win.selAcc = Math.max(0, win.selAcc - 1)
                    win.dirty = true
                    win.version++
                }
            }
        }
    }

    // ================================================================= Audio
    Component {
        id: audioPage
        ColumnLayout {
            spacing: 0
            PrefSection {
                title: "Devices"
                description: "Where you hear calls, where your voice comes from, and where incoming calls ring."
                GridLayout {
                    Layout.fillWidth: true
                    columns: 2
                    columnSpacing: Kirigami.Units.largeSpacing
                    QQC2.Label { text: "Speaker" }
                    QQC2.ComboBox {
                        Layout.fillWidth: true
                        model: ["System default"].concat(win.devices.outputs)
                        currentIndex: win.devIndex(win.devices.outputs, win.draft.speakerDevice || "")
                        onActivated: i => win.set("speakerDevice", i === 0 ? "" : win.devices.outputs[i - 1])
                    }
                    QQC2.Label { text: "Microphone" }
                    QQC2.ComboBox {
                        Layout.fillWidth: true
                        model: ["System default"].concat(win.devices.inputs)
                        currentIndex: win.devIndex(win.devices.inputs, win.draft.microphoneDevice || "")
                        onActivated: i => win.set("microphoneDevice", i === 0 ? "" : win.devices.inputs[i - 1])
                    }
                    QQC2.Label { text: "Ring on" }
                    QQC2.ComboBox {
                        Layout.fillWidth: true
                        model: ["System default"].concat(win.devices.outputs)
                        currentIndex: win.devIndex(win.devices.outputs, win.draft.ringDevice || "")
                        onActivated: i => win.set("ringDevice", i === 0 ? "" : win.devices.outputs[i - 1])
                    }
                }
            }
            PrefSection {
                title: "Ringtone"
                description: "Plays on the ring device. Defaults to Plasma's incoming call sound."
                RowLayout {
                    Layout.fillWidth: true
                    QQC2.TextField {
                        Layout.fillWidth: true
                        readOnly: true
                        text: win.draft.ringtone || "Plasma default"
                    }
                    QQC2.Button { text: "Choose…"; icon.name: "folder-open"; onClicked: ringtoneDialog.open() }
                    QQC2.Button {
                        icon.name: "edit-undo"
                        enabled: !!win.draft.ringtone
                        onClicked: { win.set("ringtone", ""); win.version++ }
                        QQC2.ToolTip.text: "Use the default"
                        QQC2.ToolTip.visible: hovered
                    }
                }
                RowLayout {
                    QQC2.Label { text: "Volume" }
                    QQC2.Slider {
                        Layout.preferredWidth: Kirigami.Units.gridUnit * 12
                        from: 0; to: 100; stepSize: 5
                        value: win.draft.ringVolume ?? 100
                        onMoved: win.set("ringVolume", Math.round(value))
                    }
                    QQC2.Label { text: Math.round(win.draft.ringVolume ?? 100) + "%"; opacity: 0.7 }
                }
                Dialogs.FileDialog {
                    id: ringtoneDialog
                    title: "Choose a ringtone"
                    nameFilters: ["Sounds (*.wav *.ogg *.oga)", "All files (*)"]
                    currentFolder: "file:///usr/share/sounds"
                    onAccepted: {
                        win.set("ringtone", decodeURIComponent(selectedFile.toString().replace("file://", "")))
                        win.version++
                    }
                }
            }
            PrefSection {
                title: "Voice processing"
                QQC2.CheckBox {
                    text: "Echo cancellation and noise suppression"
                    checked: win.draft.echoCancellation ?? true
                    onToggled: win.set("echoCancellation", checked)
                }
                QQC2.Label {
                    Layout.fillWidth: true
                    Layout.leftMargin: Kirigami.Units.gridUnit * 1.5
                    text: "Turn off with a headset if your voice sounds clipped to the other side."
                    wrapMode: Text.WordWrap
                    font: Kirigami.Theme.smallFont
                    opacity: 0.7
                }
                QQC2.CheckBox {
                    text: "Play keypad tones"
                    checked: win.draft.localDtmf ?? true
                    onToggled: win.set("localDtmf", checked)
                }
            }
            PrefSection {
                title: "Codecs"
                description: "Ticked codecs are offered in this order. Opus and G.722 sound best when the other side supports them."
                last: true
                CodecList {
                    Layout.fillWidth: true
                    enabledCodecs: win.draft.codecs || []
                    onCodecsChanged: list => win.set("codecs", list)
                }
            }
        }
    }

    // ================================================================= Calls
    Component {
        id: callsPage
        ColumnLayout {
            spacing: 0
            PrefSection {
                title: "Auto answer"
                description: "Pick up incoming calls without touching anything."
                RowLayout {
                    QQC2.ComboBox {
                        id: aa
                        Layout.preferredWidth: Kirigami.Units.gridUnit * 14
                        textRole: "t"
                        model: [{ t: "Off", v: "off" }, { t: "All calls", v: "all" }, { t: "When the server asks (intercom)", v: "header" }]
                        currentIndex: Math.max(0, ["off", "all", "header"].indexOf(win.draft.autoAnswer || "off"))
                        onActivated: i => win.set("autoAnswer", ["off", "all", "header"][i])
                    }
                    QQC2.Label { text: "after"; visible: aa.currentIndex > 0 }
                    QQC2.SpinBox {
                        visible: aa.currentIndex > 0
                        from: 0; to: 60; editable: true
                        value: win.draft.autoAnswerDelay || 0
                        onValueModified: win.set("autoAnswerDelay", value)
                    }
                    QQC2.Label { text: "seconds"; visible: aa.currentIndex > 0 }
                }
            }
            PrefSection {
                title: "Several calls"
                QQC2.CheckBox {
                    text: "Call waiting: ring for a second call while I'm on one"
                    checked: win.draft.callWaiting ?? true
                    onToggled: win.set("callWaiting", checked)
                }
                QQC2.CheckBox {
                    text: "Single call mode: a new call ends the current one"
                    checked: !!win.draft.singleCallMode
                    onToggled: win.set("singleCallMode", checked)
                }
            }
            PrefSection {
                title: "Keypad (DTMF)"
                description: "How key presses reach the other side during a call. Auto works with almost every system."
                QQC2.ComboBox {
                    Layout.preferredWidth: Kirigami.Units.gridUnit * 14
                    model: ["Auto", "RFC 2833", "SIP INFO", "In-band audio"]
                    currentIndex: Math.max(0, ["auto", "rfc2833", "info", "inband"].indexOf(win.draft.dtmfMethod || "auto"))
                    onActivated: i => win.set("dtmfMethod", ["auto", "rfc2833", "info", "inband"][i])
                }
            }
            PrefSection {
                title: "Hang up automatically"
                description: "End calls after a set time. 0 means never."
                last: true
                RowLayout {
                    QQC2.SpinBox {
                        from: 0; to: 86400; editable: true
                        value: win.draft.autoHangupSecs || 0
                        onValueModified: win.set("autoHangupSecs", value)
                    }
                    QQC2.Label { text: "seconds" }
                }
            }
        }
    }

    // ================================================================= Availability
    Component {
        id: availabilityPage
        ColumnLayout {
            spacing: 0
            PrefSection {
                title: "Do not disturb"
                description: "Incoming calls are turned away as busy. Also in the status bar and the tray menu."
                QQC2.CheckBox {
                    text: "Do not disturb"
                    checked: !!win.draft.dnd
                    onToggled: win.set("dnd", checked)
                }
            }
            PrefSection {
                title: "Call forwarding"
                description: "Send incoming calls to another number."
                RowLayout {
                    Layout.fillWidth: true
                    QQC2.ComboBox {
                        id: fwd
                        Layout.preferredWidth: Kirigami.Units.gridUnit * 11
                        model: ["Off", "Always", "When busy", "When not answered"]
                        currentIndex: Math.max(0, ["off", "always", "busy", "noanswer"].indexOf(win.draft.forwarding || "off"))
                        onActivated: i => win.set("forwarding", ["off", "always", "busy", "noanswer"][i])
                    }
                    QQC2.Label { text: "to"; visible: fwd.currentIndex > 0 }
                    QQC2.TextField {
                        Layout.fillWidth: true
                        visible: fwd.currentIndex > 0
                        placeholderText: "Number"
                        text: win.draft.forwardingNumber || ""
                        onTextEdited: win.set("forwardingNumber", text.trim())
                    }
                }
                RowLayout {
                    visible: fwd.currentIndex === 3
                    QQC2.Label { text: "after" }
                    QQC2.SpinBox {
                        from: 1; to: 120; editable: true
                        value: win.draft.forwardingDelay || 20
                        onValueModified: win.set("forwardingDelay", value)
                    }
                    QQC2.Label { text: "seconds of ringing" }
                }
            }
            PrefSection {
                title: "Deny incoming calls"
                description: "Turn calls away before they ring."
                last: true
                QQC2.ComboBox {
                    Layout.preferredWidth: Kirigami.Units.gridUnit * 14
                    model: ["No", "From other domains", "All calls"]
                    currentIndex: Math.max(0, ["", "otherdomain", "all"].indexOf(win.draft.denyIncoming || ""))
                    onActivated: i => win.set("denyIncoming", ["", "otherdomain", "all"][i])
                }
            }
        }
    }

    // ================================================================= General
    Component {
        id: generalPage
        ColumnLayout {
            spacing: 0
            PrefSection {
                title: "Startup"
                QQC2.CheckBox {
                    text: "Start MegaSIP when I log in"
                    checked: win.phone.autostartEnabled()
                    onToggled: win.phone.setAutostart(checked)
                }
                QQC2.CheckBox {
                    text: "Start in the system tray"
                    checked: !!win.draft.startMinimized
                    onToggled: win.set("startMinimized", checked)
                }
            }
            PrefSection {
                title: "Window"
                last: true
                QQC2.CheckBox {
                    text: "Bring the window to the front on an incoming call"
                    checked: win.draft.bringToFront ?? true
                    onToggled: win.set("bringToFront", checked)
                }
                QQC2.CheckBox {
                    text: "Keep the window on top"
                    checked: !!win.draft.alwaysOnTop
                    onToggled: win.set("alwaysOnTop", checked)
                }
            }
        }
    }

    // ================================================================= Network
    Component {
        id: networkPage
        ColumnLayout {
            spacing: 0
            PrefSection {
                title: "Ports"
                description: "The defaults work for almost everyone. Change them if a firewall needs fixed ports."
                RowLayout {
                    QQC2.Label { text: "SIP port" }
                    QQC2.SpinBox {
                        from: 0; to: 65535; editable: true
                        textFromValue: (v, l) => v === 0 ? "Any" : String(v)
                        valueFromText: (t, l) => t === "Any" ? 0 : parseInt(t) || 0
                        value: win.draft.sourcePort || 0
                        onValueModified: win.set("sourcePort", value)
                    }
                }
                RowLayout {
                    QQC2.Label { text: "Media (RTP) ports" }
                    QQC2.SpinBox {
                        from: 1024; to: 65000; editable: true
                        textFromValue: (v, l) => String(v)
                        value: win.draft.rtpPortMin || 10000
                        onValueModified: win.set("rtpPortMin", value)
                    }
                    QQC2.Label { text: "to" }
                    QQC2.SpinBox {
                        from: 1026; to: 65535; editable: true
                        textFromValue: (v, l) => String(v)
                        value: win.draft.rtpPortMax || 20000
                        onValueModified: win.set("rtpPortMax", value)
                    }
                }
            }
            PrefSection {
                title: "NAT"
                description: "Helps when you're behind a home router and the other side can't hear you."
                PrefField {
                    label: "STUN server"
                    QQC2.TextField {
                        Layout.preferredWidth: Kirigami.Units.gridUnit * 18
                        placeholderText: "stun.example.com:3478"
                        text: win.draft.stun || ""
                        onTextEdited: win.set("stun", text.trim())
                    }
                }
                QQC2.CheckBox {
                    text: "Use rport (RFC 3581)"
                    checked: win.draft.rport ?? true
                    onToggled: win.set("rport", checked)
                }
            }
            PrefSection {
                title: "Protocol"
                last: true
                QQC2.CheckBox {
                    text: "Look up servers with DNS SRV"
                    checked: win.draft.dnsSrv ?? true
                    onToggled: win.set("dnsSrv", checked)
                }
                PrefField {
                    label: "User agent"
                    hint: "Some providers want a specific one. Leave empty for MegaSIP's."
                    QQC2.TextField {
                        Layout.preferredWidth: Kirigami.Units.gridUnit * 18
                        placeholderText: "MegaSIP/0.1.0"
                        text: win.draft.userAgent || ""
                        onTextEdited: win.set("userAgent", text)
                    }
                }
            }
        }
    }

    // ================================================================= Logging
    Component {
        id: loggingPage
        ColumnLayout {
            spacing: 0
            PrefSection {
                title: "What to log"
                description: "SIP and audio are logged separately so one doesn't drown out the other."
                GridLayout {
                    columns: 3
                    columnSpacing: Kirigami.Units.largeSpacing
                    QQC2.Label { text: "SIP and calls" }
                    QQC2.ComboBox {
                        Layout.preferredWidth: Kirigami.Units.gridUnit * 9
                        model: win.logLevelNames
                        currentIndex: Math.max(0, win.logLevels.indexOf(win.draft.logSipLevel || "info"))
                        onActivated: i => win.set("logSipLevel", win.logLevels[i])
                    }
                    QQC2.Label { text: "Debug shows every SIP message"; opacity: 0.7; font: Kirigami.Theme.smallFont }
                    QQC2.Label { text: "Audio" }
                    QQC2.ComboBox {
                        Layout.preferredWidth: Kirigami.Units.gridUnit * 9
                        model: win.logLevelNames
                        currentIndex: Math.max(0, win.logLevels.indexOf(win.draft.logAudioLevel || "warn"))
                        onActivated: i => win.set("logAudioLevel", win.logLevels[i])
                    }
                    QQC2.Label { text: "PulseAudio, devices, echo cancellation"; opacity: 0.7; font: Kirigami.Theme.smallFont }
                }
            }
            PrefSection {
                title: "Log files"
                description: "sip.log and audio.log, kept in one folder. Each starts over after 20 MB, keeping the previous one."
                QQC2.CheckBox {
                    text: "Write log files"
                    checked: !!win.draft.logToFiles
                    onToggled: win.set("logToFiles", checked)
                }
                QQC2.Button {
                    text: "Open log folder"
                    icon.name: "folder-open"
                    onClicked: Qt.openUrlExternally("file://" + win.phone.logFolder())
                }
            }
            PrefSection {
                title: "Venator"
                description: "Send both logs to the Venator log viewer, where you can filter by target (megasip::engine, pulseaudio, …). Start Venator first; MegaSIP reconnects every few seconds."
                last: true
                QQC2.CheckBox {
                    text: "Send logs to Venator"
                    checked: !!win.draft.logToVenator
                    onToggled: win.set("logToVenator", checked)
                }
                PrefField {
                    label: "Venator address"
                    QQC2.TextField {
                        Layout.preferredWidth: Kirigami.Units.gridUnit * 14
                        placeholderText: "127.0.0.1:8362"
                        text: win.draft.venatorHost || ""
                        onTextEdited: win.set("venatorHost", text.trim())
                    }
                }
            }
        }
    }
}
