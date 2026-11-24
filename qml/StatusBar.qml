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
    signal preferencesRequested()
    signal accountRequested()

    function stateColor(s) {
        if (s === "registered") return Kirigami.Theme.positiveTextColor
        if (s === "error") return Kirigami.Theme.negativeTextColor
        if (s === "registering") return Kirigami.Theme.neutralTextColor
        return Kirigami.Theme.disabledTextColor
    }
    property var defaultAcc: {
        for (let a of accounts) if (a.id === settings.defaultAccount) return a
        return accounts.length ? accounts[0] : null
    }

    contentItem: RowLayout {
        spacing: Kirigami.Units.smallSpacing

        Rectangle {
            width: Kirigami.Units.gridUnit * 0.6
            height: width
            radius: width / 2
            color: bar.phone.dnd ? Kirigami.Theme.negativeTextColor : bar.stateColor(bar.defaultAcc ? bar.defaultAcc.state : "off")
            Layout.leftMargin: Kirigami.Units.smallSpacing
        }

        QQC2.ToolButton {
            id: accButton
            Layout.fillWidth: true
            contentItem: ColumnLayout {
                spacing: 0
                QQC2.Label {
                    Layout.fillWidth: true
                    text: bar.defaultAcc ? bar.defaultAcc.title : "No account"
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
                if (bar.accounts.length > 1) accMenu.popup()
                else if (!bar.defaultAcc || bar.defaultAcc.state !== "registered") bar.accountRequested()
            }
            QQC2.Menu {
                id: accMenu
                Repeater {
                    model: bar.accounts
                    QQC2.MenuItem {
                        required property var modelData
                        text: modelData.title + "  —  " + modelData.text
                        checkable: true
                        checked: bar.defaultAcc && modelData.id === bar.defaultAcc.id
                        onTriggered: {
                            const s = JSON.parse(bar.phone.settingsJson)
                            s.defaultAccount = modelData.id
                            bar.phone.saveSettings(JSON.stringify(s))
                        }
                    }
                }
            }
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
