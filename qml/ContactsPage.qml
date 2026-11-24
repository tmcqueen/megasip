import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import QtQuick.Dialogs as Dialogs
import org.kde.kirigami as Kirigami
import org.kde.kirigamiaddons.components as Components

Item {
    id: page
    required property var phone
    required property var contacts
    signal dialRequested(string number)

    function editNew(name, number) {
        editor.contact = { id: "", name: name || "", number: number || "", favorite: false, notes: "" }
        editor.open()
    }

    property var filtered: {
        const t = search.text.trim().toLowerCase()
        if (!t) return contacts
        return contacts.filter(c => (c.name || "").toLowerCase().includes(t) || c.number.toLowerCase().includes(t))
    }

    ColumnLayout {
        anchors.fill: parent
        spacing: 0

        RowLayout {
            Layout.fillWidth: true
            Layout.margins: Kirigami.Units.smallSpacing
            Kirigami.SearchField {
                id: search
                Layout.fillWidth: true
                placeholderText: "Search contacts"
            }
            QQC2.ToolButton {
                icon.name: "contact-new-symbolic"
                onClicked: page.editNew("", "")
                QQC2.ToolTip.text: "Add contact"
                QQC2.ToolTip.visible: hovered
            }
            QQC2.ToolButton {
                icon.name: "overflow-menu"
                onClicked: moreMenu.popup()
                QQC2.Menu {
                    id: moreMenu
                    QQC2.MenuItem { text: "Import (MicroSIP XML or CSV)…"; icon.name: "document-import"; onTriggered: importDialog.open() }
                    QQC2.MenuItem { text: "Export as CSV…"; icon.name: "document-export"; onTriggered: exportDialog.open() }
                }
            }
        }

        QQC2.ScrollView {
            Layout.fillWidth: true
            Layout.fillHeight: true
            ListView {
                id: list
                clip: true
                model: page.filtered
                delegate: QQC2.ItemDelegate {
                    id: d
                    required property var modelData
                    width: ListView.view.width
                    onClicked: page.dialRequested(modelData.number)
                    contentItem: RowLayout {
                        Components.Avatar {
                            implicitWidth: Kirigami.Units.iconSizes.medium
                            implicitHeight: implicitWidth
                            name: d.modelData.name || d.modelData.number
                        }
                        ColumnLayout {
                            Layout.fillWidth: true
                            spacing: 0
                            QQC2.Label { Layout.fillWidth: true; text: d.modelData.name || d.modelData.number; elide: Text.ElideRight }
                            QQC2.Label {
                                Layout.fillWidth: true
                                text: d.modelData.number
                                visible: !!d.modelData.name
                                font: Kirigami.Theme.smallFont
                                opacity: 0.7
                                elide: Text.ElideRight
                            }
                        }
                        QQC2.ToolButton {
                            icon.name: d.modelData.favorite ? "favorite-favorited" : "favorite"
                            onClicked: {
                                const c = Object.assign({}, d.modelData)
                                c.favorite = !c.favorite
                                page.phone.saveContact(JSON.stringify(c))
                            }
                        }
                        QQC2.ToolButton {
                            icon.name: "call-start"
                            onClicked: page.dialRequested(d.modelData.number)
                        }
                    }
                    TapHandler {
                        acceptedButtons: Qt.RightButton
                        onTapped: ctx.popup()
                    }
                    onPressAndHold: ctx.popup()
                    QQC2.Menu {
                        id: ctx
                        QQC2.MenuItem { text: "Call"; icon.name: "call-start"; onTriggered: page.dialRequested(d.modelData.number) }
                        QQC2.MenuItem { text: "Edit…"; icon.name: "document-edit"; onTriggered: { editor.contact = Object.assign({}, d.modelData); editor.open() } }
                        QQC2.MenuItem { text: "Delete"; icon.name: "edit-delete-remove"; onTriggered: page.phone.deleteContact(d.modelData.id) }
                    }
                }
                Kirigami.PlaceholderMessage {
                    anchors.centerIn: parent
                    width: parent.width - Kirigami.Units.gridUnit * 4
                    visible: list.count === 0
                    icon.name: "im-user"
                    text: search.text ? "No matches" : "No contacts yet"
                    helpfulAction: Kirigami.Action {
                        icon.name: "contact-new-symbolic"
                        text: "Add contact"
                        onTriggered: page.editNew(search.text, "")
                    }
                }
            }
        }
    }

    Kirigami.Dialog {
        id: editor
        property var contact: ({})
        title: contact.id ? "Edit contact" : "New contact"
        standardButtons: Kirigami.Dialog.Save | Kirigami.Dialog.Cancel
        padding: Kirigami.Units.largeSpacing
        onOpened: { nameField.text = contact.name || ""; numberField.text = contact.number || ""; notesField.text = contact.notes || ""; nameField.forceActiveFocus() }
        onAccepted: {
            const c = Object.assign({}, contact)
            c.name = nameField.text.trim()
            c.number = numberField.text.trim()
            c.notes = notesField.text
            page.phone.saveContact(JSON.stringify(c))
        }
        Kirigami.FormLayout {
            QQC2.TextField { id: nameField; Kirigami.FormData.label: "Name:" }
            QQC2.TextField { id: numberField; Kirigami.FormData.label: "Number:"; inputMethodHints: Qt.ImhDialableCharactersOnly }
            QQC2.TextField { id: notesField; Kirigami.FormData.label: "Notes:" }
        }
    }

    Dialogs.FileDialog {
        id: importDialog
        title: "Import contacts"
        nameFilters: ["Contacts (*.xml *.csv *.txt)", "All files (*)"]
        onAccepted: {
            const n = page.phone.importContacts(selectedFile.toString())
            imported.text = n + " contacts imported"
            imported.visible = true
        }
    }
    Dialogs.FileDialog {
        id: exportDialog
        title: "Export contacts"
        fileMode: Dialogs.FileDialog.SaveFile
        defaultSuffix: "csv"
        nameFilters: ["CSV (*.csv)"]
        onAccepted: page.phone.exportContacts(selectedFile.toString())
    }
    Kirigami.InlineMessage {
        id: imported
        anchors { left: parent.left; right: parent.right; bottom: parent.bottom; margins: Kirigami.Units.smallSpacing }
        type: Kirigami.MessageType.Positive
        showCloseButton: true
        visible: false
    }
}
