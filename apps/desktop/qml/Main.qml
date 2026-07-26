import QtQuick
import QtQuick.Controls
import QtQuick.Dialogs
import QtQuick.Layouts
import Pptalk

ApplicationWindow {
    id: window
    width: 1240
    height: 780
    minimumWidth: 900
    minimumHeight: 600
    visible: true
    title: "pptalk"
    color: "#111016"

    property color panel: "#19171F"
    property color raised: "#211E29"
    property color line: "#302C38"
    property color text: "#F4F2F7"
    property color muted: "#908B9D"
    property color accent: "#8173F2"
    property string replyMessageId: ""
    property string editMessageId: ""
    property string contextBody: ""
    property bool loadingDraft: false

    function submitComposer() {
        if (composer.text.trim().length === 0) return
        if (window.editMessageId.length > 0)
            App.editMessage(window.editMessageId, composer.text)
        else if (window.replyMessageId.length > 0)
            App.replyToMessage(window.replyMessageId, composer.text)
        else
            App.sendMessage(composer.text)
        composer.clear()
        App.saveDraft("")
        window.editMessageId = ""
        window.replyMessageId = ""
        window.contextBody = ""
    }

    function mediaChoices(kind) {
        const values = [{ "id": "", "label": "Predeterminado del sistema" }]
        for (let i = 0; i < App.mediaDevices.length; ++i) {
            if (App.mediaDevices[i].kind === kind) values.push(App.mediaDevices[i])
        }
        return values
    }

    function mediaChoiceIndex(kind, choices) {
        const selected = App.selectedMediaDevice(kind)
        for (let i = 0; i < choices.length; ++i) {
            if (choices[i].id === selected) return i
        }
        return 0
    }

    Connections {
        target: App
        function onInvitePreviewChanged() {
            if (App.invitePreviewName.length > 0) invitePreviewDialog.open()
        }
        function onConversationChanged() {
            window.loadingDraft = true
            composer.text = App.draft()
            window.loadingDraft = false
            window.replyMessageId = ""
            window.editMessageId = ""
            window.contextBody = ""
        }
        function onMessagesChanged() {
            if (App.focusedMessageId.length === 0) return
            for (let i = 0; i < App.messages.length; ++i) {
                if (App.messages[i].messageId === App.focusedMessageId) {
                    messageList.positionViewAtIndex(i, ListView.Center)
                    return
                }
            }
        }
    }

    RowLayout {
        anchors.fill: parent
        spacing: 0

        Rectangle {
            Layout.preferredWidth: 310
            Layout.fillHeight: true
            color: window.panel
            border.color: window.line

            ColumnLayout {
                anchors.fill: parent
                anchors.margins: 14
                spacing: 10

                RowLayout {
                    Layout.fillWidth: true
                    Layout.preferredHeight: 54
                    spacing: 10
                    Rectangle {
                        width: 38; height: 38; radius: 12
                        gradient: Gradient {
                            GradientStop { position: 0; color: "#9D8CFF" }
                            GradientStop { position: 1; color: "#5B4ACB" }
                        }
                        Text { anchors.centerIn: parent; text: "p"; color: "white"; font.pixelSize: 23; font.weight: Font.Bold }
                    }
                    ColumnLayout {
                        spacing: 0
                        Text { text: "pptalk"; color: window.text; font.pixelSize: 18; font.weight: Font.DemiBold }
                        Text { text: "tu red, tus conversaciones"; color: window.muted; font.pixelSize: 10 }
                    }
                    Item { Layout.fillWidth: true }
                    IconButton { glyph: "◫"; onClicked: groupDialog.open() }
                    IconButton { glyph: "+"; onClicked: inviteDialog.open() }
                }

                TextField {
                    id: conversationSearch
                    Layout.fillWidth: true
                    placeholderText: "Buscar"
                    color: window.text
                    placeholderTextColor: "#777181"
                    leftPadding: 14
                    background: Rectangle { color: "#121117"; radius: 12; border.color: window.line }
                    onTextChanged: App.search(text)
                }

                ListView {
                    Layout.fillWidth: true
                    Layout.preferredHeight: App.searchResults.length > 0 ? Math.min(180, contentHeight) : 0
                    visible: height > 0
                    clip: true
                    model: App.searchResults
                    spacing: 3
                    delegate: Rectangle {
                        required property var modelData
                        width: ListView.view.width; height: 54; radius: 9; color: searchMouse.containsMouse ? "#292534" : "#211E29"
                        Column {
                            anchors.fill: parent; anchors.margins: 8; spacing: 3
                            Text { text: modelData.author; color: window.text; font.pixelSize: 11; font.weight: Font.DemiBold }
                            Text { text: modelData.body; color: window.muted; width: parent.width; elide: Text.ElideRight; font.pixelSize: 10 }
                        }
                        MouseArea {
                            id: searchMouse
                            anchors.fill: parent
                            hoverEnabled: true
                            onClicked: App.openSearchResult(
                                modelData.conversationKey, modelData.messageId)
                        }
                    }
                }

                RowLayout {
                    Layout.fillWidth: true
                    Layout.topMargin: 6
                    Text {
                        text: App.archivedVisible ? "ARCHIVADAS Y ACTIVAS" : "CONVERSACIONES"
                        color: "#6F697A"
                        font.pixelSize: 10
                        font.letterSpacing: 1.2
                    }
                    Item { Layout.fillWidth: true }
                    ToolButton {
                        text: App.archivedVisible ? "Ocultar archivo" : "Ver archivo"
                        font.pixelSize: 10
                        onClicked: App.archivedVisible = !App.archivedVisible
                    }
                }

                ListView {
                    id: contactList
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    spacing: 5
                    clip: true
                    model: App.contacts
                    currentIndex: 0
                    delegate: Rectangle {
                        required property int index
                        required property var modelData
                        width: contactList.width
                        readonly property bool matchesSearch: conversationSearch.text.trim().length === 0 ||
                            modelData.name.toLowerCase().includes(conversationSearch.text.trim().toLowerCase())
                        visible: matchesSearch
                        height: matchesSearch ? 70 : 0
                        radius: 14
                        color: contactList.currentIndex === index ? "#292534" : (contactMouse.containsMouse ? "#221F29" : "transparent")
                        border.color: contactList.currentIndex === index ? "#403A50" : "transparent"
                        MouseArea {
                            id: contactMouse
                            anchors.fill: parent
                            hoverEnabled: true
                            onClicked: { contactList.currentIndex = index; App.selectConversation(index) }
                        }
                        Avatar { x: 11; anchors.verticalCenter: parent.verticalCenter; label: modelData.name; source: modelData.avatar || ""; accent: modelData.accent; size: 44 }
                        Column {
                            x: 67; anchors.verticalCenter: parent.verticalCenter; spacing: 5
                            Text { text: modelData.name; color: window.text; font.pixelSize: 14; font.weight: Font.Medium }
                            Text {
                                text: (modelData.pinned ? "Fijado · " : "") + (modelData.archived ? "Archivado · " : "") +
                                      (modelData.muted ? "Silenciado · " : "") + modelData.summary
                                color: window.muted; font.pixelSize: 11; elide: Text.ElideRight; width: 174
                            }
                        }
                        Rectangle {
                            visible: modelData.unread > 0
                            anchors.right: parent.right; anchors.rightMargin: 10; anchors.verticalCenter: parent.verticalCenter
                            width: 21; height: 21; radius: 11; color: window.accent
                            Text { anchors.centerIn: parent; text: modelData.unread; color: "white"; font.pixelSize: 10; font.weight: Font.Bold }
                        }
                    }
                }

                Text {
                    visible: contactList.count === 0
                    Layout.fillWidth: true
                    text: "Aún no tienes contactos.\nPulsa + para crear o aceptar una invitación."
                    color: window.muted
                    horizontalAlignment: Text.AlignHCenter
                    wrapMode: Text.Wrap
                    font.pixelSize: 12
                }

                Rectangle { Layout.fillWidth: true; height: 1; color: window.line }
                RowLayout {
                    Layout.fillWidth: true
                    Avatar { label: App.profileName; source: App.profileAvatar; accent: "#77D8B1"; size: 38 }
                    ColumnLayout {
                        spacing: 1
                        Text { text: App.profileName; color: window.text; font.pixelSize: 13; font.weight: Font.Medium }
                        Text { text: "identidad local · sin cuenta"; color: "#64CDA2"; font.pixelSize: 10 }
                    }
                    Item { Layout.fillWidth: true }
                    IconButton { glyph: "⚙"; onClicked: settingsDialog.open() }
                }
            }
        }

        Rectangle {
            Layout.fillWidth: true
            Layout.fillHeight: true
            color: "#131219"

            DropArea {
                anchors.fill: parent
                onDropped: drop => {
                    for (let i = 0; i < drop.urls.length; ++i) App.sendFile(drop.urls[i])
                    drop.acceptProposedAction()
                }
            }

            ColumnLayout {
                anchors.fill: parent
                spacing: 0

                Rectangle {
                    Layout.fillWidth: true
                    Layout.preferredHeight: 72
                    color: "#17151D"
                    border.color: window.line
                    RowLayout {
                        anchors.fill: parent
                        anchors.leftMargin: 22
                        anchors.rightMargin: 18
                        Avatar {
                            label: App.conversationName
                            source: App.contacts.length > 0 ? (App.contacts[contactList.currentIndex].avatar || "") : ""
                            accent: "#8B7CFF"; size: 40
                        }
                        ColumnLayout {
                            spacing: 1
                            Text { text: App.conversationName; color: window.text; font.pixelSize: 15; font.weight: Font.DemiBold }
                            Text { text: App.presence; color: window.muted; font.pixelSize: 11 }
                        }
                        Item { Layout.fillWidth: true }
                        Rectangle {
                            radius: 10; color: "#1F2A25"; border.color: "#2B463A"
                            implicitWidth: routeText.implicitWidth + 20; implicitHeight: 28
                            Text { id: routeText; anchors.centerIn: parent; text: "●  " + App.connectionLabel; color: "#72D3AA"; font.pixelSize: 10 }
                        }
                        IconButton { glyph: "☎"; active: App.callActive; onClicked: App.callActive ? App.leaveCall() : callMenu.open() }
                        IconButton { visible: App.conversationIsGroup; glyph: "⋯"; onClicked: manageGroupDialog.open() }
                        IconButton { visible: !App.conversationIsGroup; glyph: "⋯"; onClicked: contactMenu.open() }
                    }
                }

                Rectangle {
                    visible: App.lastError.length > 0
                    Layout.fillWidth: true
                    Layout.preferredHeight: errorText.implicitHeight + 22
                    color: "#3A2028"
                    border.color: "#6F3542"
                    RowLayout {
                        anchors.fill: parent
                        anchors.margins: 8
                        Text {
                            id: errorText
                            Layout.fillWidth: true
                            text: App.lastError
                            color: "#FFD4DC"
                            wrapMode: Text.Wrap
                            font.pixelSize: 11
                        }
                        ToolButton { text: "Cerrar"; onClicked: App.dismissError() }
                    }
                }

                Rectangle {
                    visible: App.incomingCallPending
                    Layout.fillWidth: true
                    Layout.preferredHeight: 74
                    color: "#252039"
                    border.color: "#544B78"
                    RowLayout {
                        anchors.fill: parent; anchors.margins: 14; spacing: 12
                        Text { text: "☎"; color: "#B9AEFF"; font.pixelSize: 22 }
                        ColumnLayout {
                            Text {
                                text: App.incomingCallRinging
                                    ? App.incomingCallContact + " te está llamando"
                                    : App.incomingCallContact + " ha abierto una sala de voz"
                                color: window.text; font.pixelSize: 13; font.weight: Font.DemiBold
                            }
                            Text { text: "La cámara permanece apagada hasta que tú la actives"; color: window.muted; font.pixelSize: 10 }
                        }
                        Item { Layout.fillWidth: true }
                        Button { text: "Ahora no"; onClicked: App.declineIncomingCall() }
                        Button { text: "Entrar"; onClicked: App.acceptIncomingCall() }
                    }
                }

                Rectangle {
                    visible: App.callActive
                    Layout.fillWidth: true
                    Layout.preferredHeight: 102
                    color: "#1C1925"
                    border.color: "#39334A"
                    RowLayout {
                        anchors.fill: parent; anchors.margins: 16; spacing: 12
                        Rectangle {
                            width: 54; height: 54; radius: 17; color: "#302A49"
                            Text { anchors.centerIn: parent; text: "♫"; color: "#B9AEFF"; font.pixelSize: 21 }
                        }
                        ColumnLayout {
                            Text { text: App.callState === "calling" ? "Llamando…" : (App.callState === "held" ? "Llamada retenida" : "Llamada activa"); color: window.text; font.pixelSize: 14; font.weight: Font.DemiBold }
                            Text { text: "P2P mesh · cifrado extremo a extremo"; color: window.muted; font.pixelSize: 11 }
                        }
                        Item { Layout.fillWidth: true }
                        Button {
                            visible: App.callParticipants.length > 0
                            text: "Personas · " + App.callParticipants.length
                            onClicked: callParticipantsDialog.open()
                        }
                        IconButton { glyph: App.microphoneEnabled ? "◉" : "×"; active: App.microphoneEnabled; onClicked: App.toggleMicrophone() }
                        IconButton { visible: App.callState === "connected"; glyph: "Ⅱ"; onClicked: App.holdCall() }
                        IconButton { glyph: "▣"; active: App.cameraEnabled; onClicked: App.toggleCamera() }
                        IconButton { glyph: "↗"; active: App.sharingScreen; onClicked: App.toggleScreenShare() }
                        Button {
                            text: "Salir"
                            onClicked: App.leaveCall()
                            contentItem: Text { text: parent.text; color: "#FFD6DC"; horizontalAlignment: Text.AlignHCenter; verticalAlignment: Text.AlignVCenter }
                            background: Rectangle { radius: 12; color: "#4A252D"; border.color: "#713843" }
                        }
                    }
                }

                ListView {
                    id: messageList
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    Layout.topMargin: 18
                    Layout.bottomMargin: 8
                    clip: true
                    spacing: 3
                    model: App.messages
                    delegate: ChatBubble {
                        required property var modelData
                        author: modelData.author
                        body: modelData.body
                        time: modelData.time
                        own: modelData.own
                        messageId: modelData.messageId
                        delivery: modelData.delivery
                        edited: modelData.edited
                        deleted: modelData.deleted
                        replyTo: modelData.replyTo
                        filePath: modelData.filePath
                        highlighted: modelData.messageId === App.focusedMessageId
                        localDeleteAllowed: !App.conversationIsGroup
                        onReplyRequested: id => {
                            window.replyMessageId = id
                            window.editMessageId = ""
                            window.contextBody = body
                            composer.forceActiveFocus()
                        }
                        onEditRequested: (id, currentBody) => {
                            window.editMessageId = id
                            window.replyMessageId = ""
                            window.contextBody = currentBody
                            composer.text = currentBody
                            composer.forceActiveFocus()
                        }
                        onDeleteRequested: id => App.deleteMessage(id)
                        onDeleteLocalRequested: id => App.deleteMessageLocal(id)
                        onOpenFileRequested: path => App.openMessageFile(path)
                    }
                    onCountChanged: positionViewAtEnd()
                }

                ColumnLayout {
                    Layout.fillWidth: true
                    Layout.leftMargin: 20
                    Layout.rightMargin: 20
                    Layout.bottomMargin: 18
                    spacing: 0

                    Repeater {
                        model: App.transfers
                        delegate: Rectangle {
                            required property var modelData
                            Layout.fillWidth: true
                            Layout.preferredHeight: 42
                            color: "#1C2A26"
                            radius: 10
                            RowLayout {
                                anchors.fill: parent
                                anchors.margins: 8
                                Text {
                                    Layout.fillWidth: true
                                    text: "Enviando " + modelData.fileName
                                    color: window.text
                                    elide: Text.ElideMiddle
                                    font.pixelSize: 10
                                }
                                ProgressBar {
                                    Layout.preferredWidth: 150
                                    from: 0
                                    to: 1
                                    value: modelData.progress
                                }
                                Text {
                                    text: Math.round(modelData.progress * 100) + "%"
                                    color: window.muted
                                    font.pixelSize: 10
                                }
                                Button {
                                    text: "Cancelar"
                                    flat: true
                                    visible: modelData.cancelable
                                    onClicked: App.cancelTransfer(modelData.id)
                                }
                            }
                        }
                    }

                    Rectangle {
                        visible: window.replyMessageId.length > 0 || window.editMessageId.length > 0
                        Layout.fillWidth: true
                        Layout.preferredHeight: visible ? 46 : 0
                        color: "#292534"
                        radius: 12
                        RowLayout {
                            anchors.fill: parent
                            anchors.leftMargin: 12
                            anchors.rightMargin: 8
                            Text {
                                Layout.fillWidth: true
                                text: (window.editMessageId.length > 0 ? "Editando: " : "Respondiendo a: ") +
                                      window.contextBody
                                color: window.muted
                                elide: Text.ElideRight
                                font.pixelSize: 11
                            }
                            ToolButton {
                                text: "Cancelar"
                                onClicked: {
                                    window.replyMessageId = ""
                                    window.editMessageId = ""
                                    window.contextBody = ""
                                    composer.text = App.draft()
                                }
                            }
                        }
                    }

                    Rectangle {
                        Layout.fillWidth: true
                        Layout.preferredHeight: Math.max(58, composer.contentHeight + 25)
                        radius: 17
                        color: "#211E29"
                        border.color: composer.activeFocus ? "#665B9A" : window.line
                        RowLayout {
                            anchors.fill: parent
                            anchors.leftMargin: 8
                            anchors.rightMargin: 8
                            spacing: 5
                            IconButton { glyph: "+"; onClicked: attachmentDialog.open() }
                            TextArea {
                                id: composer
                                Layout.fillWidth: true
                                placeholderText: window.editMessageId.length > 0 ? "Editar mensaje" : (window.replyMessageId.length > 0 ? "Escribe una respuesta" : "Escribe un mensaje")
                                placeholderTextColor: "#746F80"
                                color: window.text
                                wrapMode: TextEdit.Wrap
                                background: null
                                onTextChanged: {
                                    if (!window.loadingDraft && window.editMessageId.length === 0)
                                        draftTimer.restart()
                                }
                                Keys.onPressed: event => {
                                    if ((event.key === Qt.Key_Return || event.key === Qt.Key_Enter) &&
                                        !(event.modifiers & Qt.ShiftModifier)) {
                                        window.submitComposer()
                                        event.accepted = true
                                    }
                                }
                            }
                            IconButton {
                                glyph: "➤"
                                active: composer.text.trim().length > 0
                                onClicked: window.submitComposer()
                            }
                        }
                    }
                }

                Timer {
                    id: draftTimer
                    interval: 450
                    repeat: false
                    onTriggered: App.saveDraft(composer.text)
                }
            }
        }
    }

    Menu {
        id: callMenu
        MenuItem { text: "Abrir sala sin llamar"; onTriggered: App.startCall(false) }
        MenuItem { text: App.conversationIsGroup ? "Llamar al grupo" : "Llamar"; onTriggered: App.startCall(true) }
    }

    Dialog {
        id: callParticipantsDialog
        title: "Personas en la llamada"
        modal: true
        anchors.centerIn: parent
        width: 420
        background: Rectangle { color: "#211E29"; radius: 18; border.color: "#403A4B" }
        contentItem: ColumnLayout {
            spacing: 12
            Repeater {
                model: App.callParticipants
                delegate: RowLayout {
                    required property var modelData
                    Layout.fillWidth: true
                    Avatar { label: modelData.name; accent: "#8B7CFF"; size: 34 }
                    Text {
                        Layout.preferredWidth: 120
                        text: modelData.name
                        color: window.text
                        elide: Text.ElideRight
                    }
                    Slider {
                        Layout.fillWidth: true
                        from: 0
                        to: 2
                        value: modelData.volume
                        onMoved: App.setParticipantVolume(modelData.deviceId, value)
                    }
                    Text {
                        text: Math.round(modelData.volume * 100) + "%"
                        color: window.muted
                        font.pixelSize: 10
                    }
                }
            }
        }
    }

    Menu {
        id: contactMenu
        MenuItem {
            text: App.currentConversationPinned ? "Desfijar" : "Fijar conversación"
            onTriggered: App.setCurrentConversationPreferences(!App.currentConversationPinned, App.currentConversationArchived, App.currentConversationMuted)
        }
        MenuItem {
            text: App.currentConversationMuted ? "Activar avisos" : "Silenciar avisos y llamadas"
            onTriggered: App.setCurrentConversationPreferences(App.currentConversationPinned, App.currentConversationArchived, !App.currentConversationMuted)
        }
        MenuItem {
            text: App.currentConversationArchived ? "Sacar del archivo" : "Archivar"
            onTriggered: App.setCurrentConversationPreferences(App.currentConversationPinned, !App.currentConversationArchived, App.currentConversationMuted)
        }
        MenuSeparator {}
        MenuItem {
            text: App.currentContactPrivacyHidden ? "Compartir mi presencia" : "Ocultar mi presencia"
            onTriggered: App.setCurrentContactPrivacy(!App.currentContactPrivacyHidden)
        }
        MenuItem {
            text: App.currentContactVerified ? "Identidad verificada" : "Verificar identidad"
            onTriggered: verifyContactDialog.open()
        }
        MenuItem {
            text: App.currentContactBlocked ? "Desbloquear" : "Bloquear"
            onTriggered: App.setCurrentContactBlocked(!App.currentContactBlocked)
        }
        MenuSeparator {}
        MenuItem { text: "Eliminar contacto"; onTriggered: removeContactDialog.open() }
    }

    Dialog {
        id: verifyContactDialog
        title: "Verificar identidad"
        modal: true
        anchors.centerIn: parent
        width: 470
        background: Rectangle { color: "#211E29"; radius: 18; border.color: "#403A4B" }
        contentItem: ColumnLayout {
            spacing: 12
            Text {
                Layout.fillWidth: true
                text: "Compara esta huella con " + App.conversationName +
                      " por voz o en persona. Debéis ver exactamente los mismos bloques."
                color: window.muted
                wrapMode: Text.Wrap
            }
            TextArea {
                Layout.fillWidth: true
                readOnly: true
                selectByMouse: true
                text: App.currentContactFingerprint
                wrapMode: TextEdit.Wrap
                color: window.text
            }
            Button {
                Layout.alignment: Qt.AlignRight
                text: App.currentContactVerified ? "Quitar verificación" : "Coincide, marcar verificado"
                onClicked: {
                    App.setCurrentContactVerified(!App.currentContactVerified)
                    verifyContactDialog.close()
                }
            }
        }
    }

    Dialog {
        id: removeContactDialog
        title: "Eliminar contacto"
        standardButtons: Dialog.Cancel | Dialog.Ok
        anchors.centerIn: parent
        Text { text: "Se conservará el historial local. Para volver a conectar hará falta otra invitación."; color: window.text; wrapMode: Text.Wrap; width: 360 }
        onAccepted: App.removeCurrentContact()
    }

    FileDialog {
        id: attachmentDialog
        title: "Enviar archivo cifrado"
        fileMode: FileDialog.OpenFile
        onAccepted: App.sendFile(selectedFile)
    }

    Dialog {
        id: manageGroupDialog
        title: "Membresía del grupo"
        modal: true
        anchors.centerIn: parent
        width: 420
        background: Rectangle { color: "#211E29"; radius: 18; border.color: "#403A4B" }
        contentItem: ColumnLayout {
            spacing: 12
            RowLayout {
                Layout.fillWidth: true
                Button {
                    text: App.currentConversationPinned ? "Desfijar" : "Fijar"
                    onClicked: App.setCurrentConversationPreferences(!App.currentConversationPinned, App.currentConversationArchived, App.currentConversationMuted)
                }
                Button {
                    text: App.currentConversationMuted ? "Activar avisos" : "Silenciar avisos y llamadas"
                    onClicked: App.setCurrentConversationPreferences(App.currentConversationPinned, App.currentConversationArchived, !App.currentConversationMuted)
                }
                Button {
                    text: App.currentConversationArchived ? "Desarchivar" : "Archivar"
                    onClicked: App.setCurrentConversationPreferences(App.currentConversationPinned, !App.currentConversationArchived, App.currentConversationMuted)
                }
            }
            Text {
                text: App.currentGroupOwned ? "Eres propietario. Puedes gestionar miembros, administradores o transferir el grupo."
                      : (App.currentGroupAdmin ? "Eres administrador. Puedes añadir o expulsar miembros normales."
                                               : "Sólo los administradores pueden cambiar la membresía.")
                color: window.muted; wrapMode: Text.Wrap; Layout.fillWidth: true; font.pixelSize: 11
            }
            ComboBox {
                id: membershipContact
                Layout.fillWidth: true
                model: App.directContacts
                textRole: "name"
            }
            RowLayout {
                Layout.alignment: Qt.AlignRight
                Button { enabled: App.currentGroupOwned || App.currentGroupAdmin; text: "Expulsar"; onClicked: App.removeGroupMember(membershipContact.currentText) }
                Button { enabled: App.currentGroupOwned || App.currentGroupAdmin; text: "Añadir"; onClicked: App.addGroupMember(membershipContact.currentText) }
            }
            RowLayout {
                visible: App.currentGroupOwned
                Layout.alignment: Qt.AlignRight
                Button { text: "Quitar admin"; onClicked: App.setGroupAdministrator(membershipContact.currentText, false) }
                Button { text: "Hacer admin"; onClicked: App.setGroupAdministrator(membershipContact.currentText, true) }
                Button { text: "Transferir propiedad"; onClicked: App.transferGroupOwnership(membershipContact.currentText) }
            }
            Rectangle { visible: App.currentGroupOwned; Layout.fillWidth: true; height: 1; color: window.line }
            Button {
                visible: App.currentGroupOwned
                text: "Disolver grupo para todos"
                onClicked: dissolveGroupDialog.open()
            }
        }
    }

    Dialog {
        id: dissolveGroupDialog
        title: "Disolver grupo"
        standardButtons: Dialog.Cancel | Dialog.Ok
        anchors.centerIn: parent
        Text { text: "El grupo desaparecerá para todos sus miembros. El historial local no se borrará."; color: window.text; wrapMode: Text.Wrap; width: 360 }
        onAccepted: { App.dissolveCurrentGroup(); manageGroupDialog.close() }
    }

    Dialog {
        id: settingsDialog
        title: "Ajustes"
        modal: true
        anchors.centerIn: parent
        width: 450
        height: Math.min(700, window.height - 48)
        background: Rectangle { color: "#211E29"; radius: 18; border.color: "#403A4B" }
        contentItem: Flickable {
            clip: true
            contentWidth: width
            contentHeight: settingsContent.implicitHeight
            ScrollBar.vertical: ScrollBar {}
            ColumnLayout {
            id: settingsContent
            width: parent.width
            spacing: 12
            Text { text: "Perfil"; color: window.text; font.pixelSize: 13; font.weight: Font.DemiBold }
            RowLayout {
                Layout.fillWidth: true
                Avatar { label: profileName.text; source: App.profileAvatar; accent: "#77D8B1"; size: 44 }
                TextField { id: profileName; Layout.fillWidth: true; text: App.profileName; placeholderText: "Tu nombre" }
                Button { text: "Guardar"; onClicked: App.updateProfile(profileName.text, "") }
            }
            RowLayout {
                Button { text: "Elegir avatar"; onClicked: avatarDialog.open() }
                Button { text: "Quitar avatar"; enabled: App.profileAvatar.length > 0; onClicked: App.clearProfileAvatar() }
            }
            Rectangle { Layout.fillWidth: true; height: 1; color: window.line }
            RowLayout {
                visible: App.updateAvailable
                Layout.fillWidth: true
                Text { Layout.fillWidth: true; text: "Nueva versión " + App.updateVersion; color: window.text; font.pixelSize: 12 }
                Button { text: "Descargar"; onClicked: App.downloadUpdate() }
            }
            Switch {
                text: "No molestar"
                checked: App.doNotDisturb
                onToggled: App.doNotDisturb = checked
            }
            Text { text: "Todo seguirá llegando, pero no se reproducirán avisos ni timbres."; color: window.muted; wrapMode: Text.Wrap; Layout.fillWidth: true; font.pixelSize: 10 }
            Loader {
                active: App.platformSupportsAutostart
                visible: active
                source: active ? "components/DesktopAutostartSetting.qml" : ""
            }
            Rectangle { Layout.fillWidth: true; height: 1; color: window.line }
            Text { text: "Micrófono al entrar"; color: window.text; font.pixelSize: 13; font.weight: Font.DemiBold }
            ComboBox {
                Layout.fillWidth: true
                model: ["Micrófono abierto", "Pulsar para hablar"]
                currentIndex: App.voiceMode === "push_to_talk" ? 1 : 0
                onActivated: App.voiceMode = currentIndex === 1 ? "push_to_talk" : "open"
            }
            Text {
                visible: App.voiceMode === "push_to_talk"
                text: "Durante una llamada, mantén el atajo para hablar mientras pptalk tenga el foco."
                color: window.muted; font.pixelSize: 10
            }
            ComboBox {
                visible: App.voiceMode === "push_to_talk"
                Layout.fillWidth: true
                model: [
                    { "label": "Ctrl + Espacio", "value": "Ctrl+Space" },
                    { "label": "Alt + Espacio", "value": "Alt+Space" },
                    { "label": "F8", "value": "F8" }
                ]
                textRole: "label"
                currentIndex: App.pushToTalkShortcut === "Alt+Space" ? 1 :
                              (App.pushToTalkShortcut === "F8" ? 2 : 0)
                onActivated: App.pushToTalkShortcut = model[currentIndex].value
            }
            Text { text: "Micrófono"; color: window.text; font.pixelSize: 11 }
            ComboBox {
                id: microphoneDevice
                Layout.fillWidth: true
                property var choices: window.mediaChoices("audio_input")
                model: choices
                textRole: "label"
                currentIndex: window.mediaChoiceIndex("audio_input", choices)
                onActivated: App.selectMediaDevice("audio_input", choices[currentIndex].id)
            }
            RowLayout {
                Layout.fillWidth: true
                Button { text: "Probar micrófono"; onClicked: App.testMicrophone() }
                Text {
                    Layout.fillWidth: true
                    text: App.microphoneTestStatus
                    color: App.microphoneTestStatus === "Micrófono listo." ? "#77D8B1" : window.muted
                    wrapMode: Text.Wrap
                    font.pixelSize: 10
                }
            }
            Text { text: "Altavoces o auriculares"; color: window.text; font.pixelSize: 11 }
            ComboBox {
                id: outputDevice
                Layout.fillWidth: true
                property var choices: window.mediaChoices("audio_output")
                model: choices
                textRole: "label"
                currentIndex: window.mediaChoiceIndex("audio_output", choices)
                onActivated: App.selectMediaDevice("audio_output", choices[currentIndex].id)
            }
            Text { text: "Cámara"; color: window.text; font.pixelSize: 11 }
            ComboBox {
                id: cameraDevice
                Layout.fillWidth: true
                property var choices: window.mediaChoices("camera")
                model: choices
                textRole: "label"
                currentIndex: window.mediaChoiceIndex("camera", choices)
                onActivated: App.selectMediaDevice("camera", choices[currentIndex].id)
            }
            Rectangle { Layout.fillWidth: true; height: 1; color: window.line }
            Text { text: "Calidad de cámara y pantalla"; color: window.text; font.pixelSize: 13; font.weight: Font.DemiBold }
            ComboBox {
                id: videoQuality
                Layout.fillWidth: true
                model: ["Automática", "720p · 30 fps", "1080p · 30 fps", "1440p · 60 fps", "4K · 60 fps"]
                onActivated: App.configureVideoQuality(currentIndex)
            }
            Text {
                text: videoQuality.currentIndex === 0
                    ? "El modo automático puede adaptarse a la red."
                    : "El modo manual falla de forma visible si el equipo no puede cumplirlo; nunca reduce la calidad en silencio."
                color: window.muted; wrapMode: Text.Wrap; Layout.fillWidth: true; font.pixelSize: 10
            }
            Rectangle { Layout.fillWidth: true; height: 1; color: window.line }
            Text { text: "Vincular otro dispositivo"; color: window.text; font.pixelSize: 13; font.weight: Font.DemiBold }
            TextField { id: deviceLabel; Layout.fillWidth: true; placeholderText: "Nombre, por ejemplo Portátil" }
            RowLayout {
                Layout.fillWidth: true
                Button { text: "Generar enlace (10 min)"; onClicked: App.createDeviceLink(deviceLabel.text) }
                Button { text: "Copiar"; enabled: App.deviceLink.length > 0; onClicked: App.copyDeviceLink() }
            }
            TextArea { visible: App.deviceLink.length > 0; Layout.fillWidth: true; readOnly: true; text: App.deviceLink; wrapMode: TextEdit.WrapAnywhere; color: window.muted; background: null }
            Text { text: "Dispositivos autorizados"; color: window.text; font.pixelSize: 12; font.weight: Font.DemiBold }
            Repeater {
                model: App.devices
                delegate: RowLayout {
                    required property var modelData
                    Layout.fillWidth: true
                    Text {
                        Layout.fillWidth: true
                        text: modelData.label + (modelData.current ? " · este dispositivo" :
                              (modelData.active ? " · activo" : " · revocado"))
                        color: modelData.active ? window.text : window.muted
                        font.pixelSize: 11
                    }
                    Button {
                        visible: modelData.active && !modelData.current
                        text: "Revocar"
                        onClicked: App.revokeDevice(modelData.id)
                    }
                }
            }
            Rectangle { Layout.fillWidth: true; height: 1; color: window.line }
            Text { text: "Protección local"; color: window.text; font.pixelSize: 13; font.weight: Font.DemiBold }
            Text {
                Layout.fillWidth: true
                text: App.secureStorageEnabled
                    ? "La clave del historial está protegida por el almacén seguro del sistema."
                    : "La clave del historial sigue dentro del perfil. Puedes moverla al almacén seguro del sistema sin cambiar tu identidad."
                color: App.secureStorageEnabled ? "#77D8B1" : window.muted
                wrapMode: Text.Wrap
                font.pixelSize: 10
            }
            Button {
                text: App.secureStorageEnabled ? "Protección activada" : "Proteger con el sistema"
                enabled: !App.secureStorageEnabled
                onClicked: App.protectLocalSecrets()
            }
            Rectangle { Layout.fillWidth: true; height: 1; color: window.line }
            Text { text: "Copia cifrada de identidad"; color: window.text; font.pixelSize: 13; font.weight: Font.DemiBold }
            Text {
                Layout.fillWidth: true
                text: "Guarda tu identidad, contactos y grupos en un archivo protegido. El historial y los adjuntos permanecen en este equipo."
                color: window.muted
                wrapMode: Text.Wrap
                font.pixelSize: 10
            }
            TextField {
                id: backupPassphrase
                Layout.fillWidth: true
                echoMode: TextInput.Password
                placeholderText: "Frase para la copia (mínimo 10 caracteres)"
            }
            Button {
                text: "Guardar copia cifrada"
                enabled: backupPassphrase.text.length >= 10
                onClicked: backupExportDialog.open()
            }
            Text {
                visible: App.backupStatus.length > 0
                Layout.fillWidth: true
                text: App.backupStatus
                color: "#77D8B1"
                wrapMode: Text.Wrap
                font.pixelSize: 10
            }
            }
        }
    }

    FileDialog {
        id: avatarDialog
        title: "Elegir avatar"
        nameFilters: ["Imágenes (*.png *.jpg *.jpeg *.webp)"]
        fileMode: FileDialog.OpenFile
        onAccepted: App.updateProfile(profileName.text, selectedFile)
    }

    FileDialog {
        id: backupExportDialog
        title: "Guardar copia cifrada"
        fileMode: FileDialog.SaveFile
        nameFilters: ["Copias de pptalk (*.pptalk-backup)"]
        onAccepted: {
            App.exportIdentityBackup(selectedFile, backupPassphrase.text)
            backupPassphrase.clear()
        }
    }

    Dialog {
        id: groupDialog
        property var selectedNames: []
        function toggleMember(name, selected) {
            let next = selectedNames.slice()
            const index = next.indexOf(name)
            if (selected && index < 0) next.push(name)
            if (!selected && index >= 0) next.splice(index, 1)
            selectedNames = next
        }
        title: "Nuevo grupo privado"
        modal: true
        anchors.centerIn: parent
        width: 430
        background: Rectangle { color: "#211E29"; radius: 18; border.color: "#403A4B" }
        contentItem: ColumnLayout {
            spacing: 12
            TextField { id: groupName; Layout.fillWidth: true; placeholderText: "Nombre del grupo" }
            Text { text: "Elige participantes"; color: window.text; font.weight: Font.DemiBold }
            ScrollView {
                Layout.fillWidth: true
                Layout.preferredHeight: Math.min(180, App.directContacts.length * 42)
                ColumnLayout {
                    width: parent.width
                    Repeater {
                        model: App.directContacts
                        delegate: CheckBox {
                            required property var modelData
                            Layout.fillWidth: true
                            text: modelData.name
                            checked: groupDialog.selectedNames.indexOf(modelData.name) >= 0
                            onToggled: groupDialog.toggleMember(modelData.name, checked)
                        }
                    }
                }
            }
            Text { text: "Máximo 16 miembros en el chat y 8 en llamada. Los nuevos miembros no reciben el historial anterior."; color: window.muted; wrapMode: Text.Wrap; Layout.fillWidth: true; font.pixelSize: 11 }
            Button {
                text: "Crear grupo privado"
                enabled: groupName.text.trim().length > 0 && groupDialog.selectedNames.length > 0
                Layout.alignment: Qt.AlignRight
                onClicked: {
                    App.createGroup(groupName.text, groupDialog.selectedNames.join(","))
                    groupName.clear()
                    groupDialog.selectedNames = []
                    groupDialog.close()
                }
            }
        }
    }

    Dialog {
        id: inviteDialog
        title: "Invitar a un contacto"
        modal: true
        anchors.centerIn: parent
        width: 470
        onOpened: App.createInvite()
        background: Rectangle { color: "#211E29"; radius: 18; border.color: "#403A4B" }
        contentItem: ColumnLayout {
            spacing: 14
            Text { text: "Este enlace caduca y solo puede usarse una vez."; color: window.muted; font.pixelSize: 12 }
            Image {
                visible: App.inviteQr.length > 0
                Layout.alignment: Qt.AlignHCenter
                Layout.preferredWidth: 180
                Layout.preferredHeight: 180
                source: App.inviteQr
                fillMode: Image.PreserveAspectFit
            }
            TextField { Layout.fillWidth: true; readOnly: true; text: App.inviteLink; color: window.text; background: Rectangle { color: "#141219"; radius: 10; border.color: window.line } }
            Rectangle { Layout.fillWidth: true; height: 1; color: window.line }
            Text { text: "O pega una invitación que te hayan enviado:"; color: window.muted; font.pixelSize: 12 }
            TextField {
                id: incomingInvite
                Layout.fillWidth: true
                placeholderText: "pptalk://contact/v1#..."
                color: window.text
                background: Rectangle { color: "#141219"; radius: 10; border.color: window.line }
            }
            RowLayout {
                Button {
                    text: "Revisar invitación"
                    enabled: incomingInvite.text.trim().length > 0
                    onClicked: { App.acceptInvite(incomingInvite.text); incomingInvite.clear() }
                }
                Item { Layout.fillWidth: true }
                Button { text: "Copiar enlace"; onClicked: App.copyInvite(); background: Rectangle { color: window.accent; radius: 11 } }
            }
        }
    }

    Dialog {
        id: invitePreviewDialog
        title: "Aceptar contacto"
        modal: true
        anchors.centerIn: parent
        standardButtons: Dialog.Cancel | Dialog.Ok
        background: Rectangle { color: "#211E29"; radius: 18; border.color: "#403A4B" }
        contentItem: ColumnLayout {
            spacing: 10
            Text { text: App.invitePreviewName; color: window.text; font.pixelSize: 18; font.weight: Font.DemiBold }
            Text { text: "Caduca: " + App.invitePreviewExpiry; color: window.muted; font.pixelSize: 11 }
            Text {
                Layout.preferredWidth: 380
                text: "Acepta solo si recibiste este enlace por un canal de confianza. pptalk no compara huellas manualmente."
                color: window.muted; wrapMode: Text.Wrap; font.pixelSize: 11
            }
        }
        onAccepted: { App.confirmInvite(); inviteDialog.close() }
    }

    Onboarding {
        anchors.fill: parent
        visible: App.onboardingRequired
        z: 1000
    }
}
