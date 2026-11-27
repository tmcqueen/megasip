import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

// Account status + quick toggles (MicroSIP's top bar: account, DND, auto-answer).
QQC2.ToolBar {
    id: bar
    required property var phone
    required property var accounts
    required property var settings
    property var activeAcc: null
    property var pickable: []
    property var inUse: []
    property var accTitle: a => a.label
    signal preferencesRequested()
    signal accountRequested()
    signal menuOpening()

    function stateColor(s) {
        if (s === "registered") return Kirigami.Theme.positiveTextColor
        if (s === "error") return Kirigami.Theme.negativeTextColor
        if (s === "registering") return Kirigami.Theme.neutralTextColor
        return Kirigami.Theme.disabledTextColor
    }
    // Registration state of this window's account.
    property var activeState: {
        if (!activeAcc) return null
        for (let a of accounts) if (a.id === activeAcc.id) return a
        return null
    }

    contentItem: RowLayout {
        spacing: Kirigami.Units.smallSpacing

        Rectangle {
            width: Kirigami.Units.gridUnit * 0.6
            height: width
            radius: width / 2
            color: bar.phone.dnd ? Kirigami.Theme.negativeTextColor : bar.stateColor(bar.activeState ? bar.activeState.state : "off")
            Layout.leftMargin: Kirigami.Units.smallSpacing
        }

        QQC2.ToolButton {
            id: accButton
            Layout.fillWidth: true
            contentItem: ColumnLayout {
                spacing: 0
                QQC2.Label {
                    Layout.fillWidth: true
                    text: bar.activeAcc ? bar.accTitle(bar.activeAcc) : "No account active"
                    font.weight: Font.DemiBold
                    elide: Text.ElideRight
                }
                QQC2.Label {
                    Layout.fillWidth: true
                    text: bar.phone.dnd ? "Do not disturb" : bar.phone.statusText
                    font: Kirigami.Theme.smallFont
                    opacity: 0.75
                    elide: Text.ElideRight
                }
            }
            onClicked: {
                if (bar.pickable.length === 0) { bar.accountRequested(); return }
                bar.menuOpening()
                accMenu.popup()
            }
            QQC2.Menu {
                id: accMenu
                Repeater {
                    model: bar.pickable
                    QQC2.MenuItem {
                        required property var modelData
                        readonly property bool elsewhere: bar.inUse.includes(modelData.id)
                        readonly property bool mine: !!bar.activeAcc && modelData.id === bar.activeAcc.id
                        text: bar.accTitle(modelData) + (elsewhere ? "  —  in another window"
                              : mine ? "" : bar.activeAcc ? "  —  open in a new window" : "")
                        checkable: true
                        checked: mine
                        enabled: !elsewhere
                        onTriggered: {
                            bar.phone.pickAccount(modelData.id)
                            checked = Qt.binding(() => mine)
                        }
                    }
                }
                QQC2.MenuSeparator { visible: !!bar.activeAcc }
                QQC2.MenuItem {
                    visible: !!bar.activeAcc
                    text: bar.activeAcc ? "Deactivate " + bar.accTitle(bar.activeAcc) : ""
                    icon.name: "system-log-out"
                    onTriggered: bar.phone.deactivateAccount()
                }
            }
        }

        // --source: what's being sent instead of the microphone.
        Kirigami.Chip {
            visible: bar.phone.sourceLabel !== ""
            closable: false
            checkable: false
            icon.name: "audio-x-generic"
            text: "Source: " + bar.phone.sourceLabel
            Layout.maximumWidth: Kirigami.Units.gridUnit * 9
            QQC2.ToolTip.text: "Sending " + bar.phone.sourceDetail + " instead of the microphone (--source)"
            QQC2.ToolTip.visible: hovered
        }

        QQC2.ToolButton {
            icon.name: "notifications-disabled"
            checkable: true
            checked: bar.phone.dnd
            onToggled: bar.phone.setDndOn(checked)
            QQC2.ToolTip.text: "Do not disturb (DND)"
            QQC2.ToolTip.visible: hovered
            display: QQC2.AbstractButton.IconOnly
            text: "DND"
        }
        QQC2.ToolButton {
            icon.name: "settings-configure"
            onClicked: bar.preferencesRequested()
            display: QQC2.AbstractButton.IconOnly
            text: "Preferences"
            QQC2.ToolTip.text: "Preferences (Ctrl+,)"
            QQC2.ToolTip.visible: hovered
        }
        QQC2.ToolButton {
            text: "AA"
            checkable: true
            checked: bar.phone.autoAnswer
            onToggled: bar.phone.setAutoAnswerOn(checked)
            QQC2.ToolTip.text: "Auto answer"
            QQC2.ToolTip.visible: hovered
        }
    }
}
