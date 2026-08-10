import QtQuick
import QtQuick.Controls
import QtQuick.Dialogs
import QtQuick.Layouts
import Pptalk

ApplicationWindow {
    id: window
    width: 1280
    height: 800
    minimumWidth: 940
    minimumHeight: 640
    visible: true
    title: App.callOngoing ? "pptalk · " + App.callContact : "pptalk"
    color: Theme.canvas

    property string replyMessageId: ""
    property string editMessageId: ""
    property string contextBody: ""
    property bool loadingDraft: false

    Shortcut { sequences: [StandardKey.Find]; onActivated: conversationSearch.forceActiveFocus() }
    Shortcut { sequence: "Ctrl+,"; onActivated: settingsDrawer.open() }
    Shortcut { sequence: "Ctrl+N"; onActivated: newConversationMenu.open() }

    function submitComposer() {
        const body = composer.text.trim()
        if (body.length === 0 || App.contacts.length === 0) return
        if (window.editMessageId.length > 0) App.editMessage(window.editMessageId, body)
        else if (window.replyMessageId.length > 0) App.replyToMessage(window.replyMessageId, body)
        else App.sendMessage(body)
        composer.clear()
        App.saveDraft("")
        cancelComposerContext()
    }

    function cancelComposerContext() {
        window.editMessageId = ""
        window.replyMessageId = ""
        window.contextBody = ""
    }

    function replyInfo(messageId) {
        for (let i = 0; i < App.messages.length; ++i) {
            if (App.messages[i].messageId === messageId)
                return { "author": App.messages[i].author, "body": App.messages[i].body }
        }
        return { "author": "", "body": "" }
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
            window.cancelComposerContext()
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
            Layout.preferredWidth: Theme.sidebarWidth
            Layout.fillHeight: true
            color: Theme.sidebar
            border.color: Theme.border

            ColumnLayout {
                anchors.fill: parent
                anchors.leftMargin: 14
                anchors.rightMargin: 14
                anchors.topMargin: 14
                anchors.bottomMargin: 12
                spacing: 10

                RowLayout {
                    Layout.fillWidth: true
                    Layout.preferredHeight: 48
                    spacing: 10
                    Rectangle {
                        width: 38; height: 38; radius: 12
                        color: Theme.accentStrong
                        Text { anchors.centerIn: parent; text: "p"; color: "white"; font.pixelSize: 23; font.weight: Font.Bold }
                    }
                    ColumnLayout {
                        spacing: -1
                        Text { text: "pptalk"; color: Theme.text; font.pixelSize: 17; font.weight: Font.DemiBold }
                        Text { text: "privado por diseño"; color: Theme.textSubtle; font.pixelSize: 10 }
                    }
                    Item { Layout.fillWidth: true }
                    ActionButton { text: "Nuevo"; iconName: "plus"; kind: "primary"; compact: true; onClicked: newConversationMenu.open() }
                }

                Item {
                    Layout.fillWidth: true
                    Layout.preferredHeight: Theme.controlHeight
                    AppTextField {
                        id: conversationSearch
                        anchors.fill: parent
                        leftPadding: 39
                        placeholderText: "Buscar chats y mensajes"
                        onTextChanged: App.search(text)
                        Keys.onEscapePressed: { clear(); App.clearSearch() }
                    }
                    AppIcon { anchors.left: parent.left; anchors.leftMargin: 13; anchors.verticalCenter: parent.verticalCenter; name: "search"; width: 17; height: 17; color: conversationSearch.activeFocus ? Theme.accent : Theme.textSubtle }
                }

                Rectangle {
                    Layout.fillWidth: true
                    Layout.preferredHeight: App.searchResults.length > 0 ? Math.min(190, searchResults.contentHeight + 10) : 0
                    visible: height > 0
                    color: Theme.surface
                    border.color: Theme.border
                    radius: Theme.radius
                    ListView {
                        id: searchResults
                        anchors.fill: parent
                        anchors.margins: 5
                        clip: true
                        spacing: 2
                        model: App.searchResults
                        delegate: Rectangle {
                            required property var modelData
                            width: ListView.view.width; height: 54; radius: Theme.radiusSmall
                            color: resultMouse.containsMouse ? Theme.surfaceHigh : "transparent"
                            activeFocusOnTab: true
                            Accessible.role: Accessible.Button
                            Accessible.name: "Resultado de " + modelData.author + ": " + modelData.body
                            Keys.onReturnPressed: { App.openSearchResult(modelData.conversationKey, modelData.messageId); conversationSearch.clear() }
                            Keys.onSpacePressed: { App.openSearchResult(modelData.conversationKey, modelData.messageId); conversationSearch.clear() }
                            Column {
                                anchors.fill: parent; anchors.margins: 8; spacing: 3
                                Text { text: modelData.author; color: Theme.text; font.pixelSize: 11; font.weight: Font.DemiBold }
                                Text { text: modelData.body; color: Theme.textMuted; width: parent.width; elide: Text.ElideRight; font.pixelSize: 10 }
                            }
                            MouseArea { id: resultMouse; anchors.fill: parent; hoverEnabled: true; cursorShape: Qt.PointingHandCursor; onClicked: { App.openSearchResult(modelData.conversationKey, modelData.messageId); conversationSearch.clear() } }
                        }
                    }
                }

                RowLayout {
                    Layout.fillWidth: true
                    Layout.topMargin: 5
                    Text { text: App.archivedVisible ? "TODAS LAS CONVERSACIONES" : "CONVERSACIONES"; color: Theme.textSubtle; font.pixelSize: 10; font.letterSpacing: 1.1; font.weight: Font.DemiBold }
                    Item { Layout.fillWidth: true }
                    ActionButton { text: App.archivedVisible ? "Ocultar archivo" : "Ver archivo"; kind: "ghost"; compact: true; onClicked: App.archivedVisible = !App.archivedVisible }
                }

                ListView {
                    id: contactList
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    spacing: 4
                    clip: true
                    model: App.contacts
                    currentIndex: App.selectedConversationIndex
                    boundsBehavior: Flickable.StopAtBounds
                    ScrollBar.vertical: ScrollBar {}
                    delegate: Rectangle {
                        id: conversationRow
                        required property int index
                        required property var modelData
                        readonly property bool matchesSearch: conversationSearch.text.trim().length === 0 || modelData.name.toLowerCase().includes(conversationSearch.text.trim().toLowerCase())
                        width: contactList.width
                        height: matchesSearch ? 66 : 0
                        visible: matchesSearch
                        activeFocusOnTab: matchesSearch
                        Accessible.role: Accessible.Button
                        Accessible.name: modelData.name + ". " + modelData.summary
                        radius: Theme.radius
                        color: contactList.currentIndex === index ? Theme.surfaceHigh : (contactMouse.containsMouse ? Theme.surface : "transparent")
                        border.color: activeFocus ? Theme.accent : (contactList.currentIndex === index ? Theme.borderStrong : "transparent")
                        Keys.onReturnPressed: App.selectConversation(index)
                        Keys.onSpacePressed: App.selectConversation(index)
                        MouseArea { id: contactMouse; anchors.fill: parent; hoverEnabled: true; cursorShape: Qt.PointingHandCursor; onClicked: App.selectConversation(index) }
                        Avatar { x: 10; anchors.verticalCenter: parent.verticalCenter; label: modelData.name; source: modelData.avatar || ""; accent: modelData.accent; size: 42 }
                        Column {
                            x: 62; anchors.verticalCenter: parent.verticalCenter; spacing: 4; width: parent.width - 98
                            Row {
                                width: parent.width; spacing: 6
                                Text { width: parent.width - statusIcons.width - 6; text: modelData.name; color: Theme.text; font.pixelSize: 13; font.weight: Font.DemiBold; elide: Text.ElideRight }
                                Row {
                                    id: statusIcons; spacing: 3
                                    AppIcon { visible: modelData.pinned; name: "pin"; width: 12; height: 12; color: Theme.textSubtle }
                                    AppIcon { visible: modelData.muted; name: "bell-off"; width: 12; height: 12; color: Theme.textSubtle }
                                }
                            }
                            Text { text: modelData.summary; color: Theme.textMuted; width: parent.width; elide: Text.ElideRight; font.pixelSize: 10 }
                        }
                        Rectangle {
                            visible: modelData.unread > 0
                            anchors.right: parent.right; anchors.rightMargin: 10; anchors.verticalCenter: parent.verticalCenter
                            width: 22; height: 22; radius: 11; color: Theme.accentStrong
                            Text { anchors.centerIn: parent; text: modelData.unread > 99 ? "99+" : modelData.unread; color: "white"; font.pixelSize: 9; font.weight: Font.Bold }
                        }
                    }
                }

                ColumnLayout {
                    visible: contactList.count === 0
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    spacing: 10
                    Item { Layout.fillHeight: true }
                    AppIcon { Layout.alignment: Qt.AlignHCenter; name: "users"; width: 30; height: 30; color: Theme.textSubtle }
                    Text { Layout.fillWidth: true; text: "Tu gente aparecerá aquí"; color: Theme.text; horizontalAlignment: Text.AlignHCenter; font.pixelSize: 13; font.weight: Font.DemiBold }
                    Text { Layout.fillWidth: true; text: "Añade un contacto para empezar."; color: Theme.textMuted; horizontalAlignment: Text.AlignHCenter; font.pixelSize: 11 }
                    ActionButton { Layout.alignment: Qt.AlignHCenter; text: "Añadir contacto"; iconName: "user-add"; kind: "primary"; compact: true; onClicked: inviteDialog.open() }
                    Item { Layout.fillHeight: true }
                }

                Rectangle { Layout.fillWidth: true; height: 1; color: Theme.border }
                Rectangle {
                    Layout.fillWidth: true
                    Layout.preferredHeight: 56
                    radius: Theme.radius
                    color: profileMouse.containsMouse ? Theme.surface : "transparent"
                    activeFocusOnTab: true
                    Accessible.role: Accessible.Button
                    Accessible.name: "Abrir Ajustes para " + App.profileName
                    Keys.onReturnPressed: settingsDrawer.open()
                    Keys.onSpacePressed: settingsDrawer.open()
                    MouseArea { id: profileMouse; anchors.fill: parent; hoverEnabled: true; cursorShape: Qt.PointingHandCursor; onClicked: settingsDrawer.open() }
                    RowLayout {
                        anchors.fill: parent; anchors.leftMargin: 8; anchors.rightMargin: 7; spacing: 10
                        Avatar { label: App.profileName; source: App.profileAvatar; accent: Theme.positive; size: 38 }
                        ColumnLayout {
                            spacing: 1
                            Text { text: App.profileName; color: Theme.text; font.pixelSize: 12; font.weight: Font.DemiBold; elide: Text.ElideRight; Layout.maximumWidth: 150 }
                            Text { text: "Identidad local"; color: Theme.positive; font.pixelSize: 10 }
                        }
                        Item { Layout.fillWidth: true }
                        AppIcon { name: "settings"; width: 18; height: 18; color: Theme.textMuted }
                    }
                }
            }
        }

        Rectangle {
            Layout.fillWidth: true
            Layout.fillHeight: true
            color: Theme.canvas

            DropArea {
                anchors.fill: parent
                onDropped: drop => {
                    if (App.contacts.length === 0) return
                    for (let i = 0; i < drop.urls.length; ++i) App.sendFile(drop.urls[i])
                    drop.acceptProposedAction()
                }
            }

            ColumnLayout {
                anchors.fill: parent
                spacing: 0

                Rectangle {
                    visible: App.contacts.length > 0
                    Layout.fillWidth: true
                    Layout.preferredHeight: visible ? 72 : 0
                    color: Theme.sidebar
                    border.color: Theme.border
                    RowLayout {
                        anchors.fill: parent; anchors.leftMargin: 22; anchors.rightMargin: 18; spacing: 11
                        Avatar { label: App.conversationName; source: App.contacts.length > 0 ? (App.contacts[App.selectedConversationIndex].avatar || "") : ""; accent: App.contacts.length > 0 ? App.contacts[App.selectedConversationIndex].accent : Theme.accent; size: 40 }
                        ColumnLayout {
                            spacing: 1
                            Row {
                                spacing: 7
                                Text { text: App.conversationName; color: Theme.text; font.pixelSize: 14; font.weight: Font.DemiBold }
                                AppIcon { visible: App.currentContactVerified; name: "shield"; width: 14; height: 14; color: Theme.positive }
                            }
                            Text { text: App.presence; color: Theme.textMuted; font.pixelSize: 10 }
                        }
                        Item { Layout.fillWidth: true }
                        Rectangle {
                            radius: 12; color: Theme.positiveSoft; border.color: "#285246"
                            implicitWidth: routeLabel.implicitWidth + 20; implicitHeight: 30
                            Text { id: routeLabel; anchors.centerIn: parent; text: "●  " + App.connectionLabel; color: Theme.positive; font.pixelSize: 10; font.weight: Font.Medium }
                        }
                        ActionButton { text: App.callOngoing ? "En llamada" : "Llamar"; iconName: "phone"; compact: true; enabled: !App.callOngoing; onClicked: callMenu.open() }
                        IconButton { iconName: "more"; description: App.conversationIsGroup ? "Gestionar grupo" : "Opciones del contacto"; onClicked: App.conversationIsGroup ? manageGroupDialog.open() : contactMenu.open() }
                    }
                }

                Rectangle {
                    visible: App.lastError.length > 0
                    Layout.fillWidth: true
                    Layout.preferredHeight: visible ? errorRow.implicitHeight + 22 : 0
                    color: Theme.dangerSoft
                    border.color: "#68313C"
                    RowLayout {
                        id: errorRow
                        anchors.fill: parent; anchors.leftMargin: 18; anchors.rightMargin: 12; spacing: 10
                        AppIcon { name: "info"; width: 18; height: 18; color: Theme.danger }
                        Text { Layout.fillWidth: true; text: App.lastError; color: "#FFDDE2"; wrapMode: Text.Wrap; font.pixelSize: 11 }
                        ActionButton { text: "Cerrar"; compact: true; kind: "ghost"; onClicked: App.dismissError() }
                    }
                }

                Rectangle {
                    visible: App.incomingCallPending
                    Layout.fillWidth: true
                    Layout.preferredHeight: visible ? 82 : 0
                    color: "#182438"
                    border.color: "#344D72"
                    RowLayout {
                        anchors.fill: parent; anchors.leftMargin: 20; anchors.rightMargin: 16; spacing: 12
                        Rectangle { width: 44; height: 44; radius: 14; color: Theme.accentSoft; AppIcon { anchors.centerIn: parent; name: "phone"; width: 20; height: 20; color: Theme.accent } }
                        ColumnLayout {
                            Text { text: App.incomingCallRinging ? App.incomingCallContact + " te está llamando" : App.incomingCallContact + " ha abierto una sala"; color: Theme.text; font.pixelSize: 13; font.weight: Font.DemiBold }
                            Text { text: "Tu cámara permanece apagada hasta que la actives"; color: Theme.textMuted; font.pixelSize: 10 }
                        }
                        Item { Layout.fillWidth: true }
                        ActionButton { text: "Ahora no"; compact: true; kind: "ghost"; onClicked: App.declineIncomingCall() }
                        ActionButton { text: "Entrar"; iconName: "phone"; compact: true; kind: "primary"; onClicked: App.acceptIncomingCall() }
                    }
                }

                CallPanel {
                    visible: App.callOngoing
                    Layout.fillWidth: true
                    Layout.leftMargin: 18
                    Layout.rightMargin: 18
                    Layout.topMargin: visible ? 14 : 0
                    Layout.preferredHeight: visible ? implicitHeight : 0
                    onShowParticipants: callParticipantsDialog.open()
                }

                Item {
                    visible: App.contacts.length === 0
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    ColumnLayout {
                        anchors.centerIn: parent
                        width: Math.min(430, parent.width - 80)
                        spacing: 12
                        Rectangle { Layout.alignment: Qt.AlignHCenter; width: 70; height: 70; radius: 22; color: Theme.surface; border.color: Theme.border; AppIcon { anchors.centerIn: parent; name: "shield"; width: 30; height: 30; color: Theme.accent } }
                        Text { Layout.fillWidth: true; text: "Un lugar tranquilo para tu gente"; color: Theme.text; horizontalAlignment: Text.AlignHCenter; font.pixelSize: 21; font.weight: Font.DemiBold }
                        Text { Layout.fillWidth: true; text: "Tus conversaciones viven en tus dispositivos y viajan cifradas. Añade a alguien de confianza para empezar."; color: Theme.textMuted; horizontalAlignment: Text.AlignHCenter; wrapMode: Text.Wrap; font.pixelSize: 12; lineHeight: 1.25 }
                        ActionButton { Layout.alignment: Qt.AlignHCenter; Layout.topMargin: 8; text: "Añadir mi primer contacto"; iconName: "user-add"; kind: "primary"; onClicked: inviteDialog.open() }
                    }
                }

                ListView {
                    id: messageList
                    visible: App.contacts.length > 0
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    Layout.topMargin: 12
                    Layout.bottomMargin: 6
                    leftMargin: 12
                    rightMargin: 12
                    clip: true
                    spacing: 2
                    boundsBehavior: Flickable.StopAtBounds
                    model: App.messages
                    property bool followNewest: true
                    ScrollBar.vertical: ScrollBar {}
                    delegate: ChatBubble {
                        required property var modelData
                        property var replyData: window.replyInfo(modelData.replyTo)
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
                        replyAuthor: replyData.author
                        replyBody: replyData.body
                        highlighted: modelData.messageId === App.focusedMessageId
                        localDeleteAllowed: !App.conversationIsGroup
                        onReplyRequested: id => { window.replyMessageId = id; window.editMessageId = ""; window.contextBody = body; composer.forceActiveFocus() }
                        onEditRequested: (id, currentBody) => { window.editMessageId = id; window.replyMessageId = ""; window.contextBody = currentBody; composer.text = currentBody; composer.forceActiveFocus() }
                        onDeleteRequested: id => App.deleteMessage(id)
                        onDeleteLocalRequested: id => App.deleteMessageLocal(id)
                        onOpenFileRequested: path => App.openMessageFile(path)
                    }
                    onMovementEnded: followNewest = atYEnd
                    onCountChanged: if (followNewest) Qt.callLater(positionViewAtEnd)

                    ColumnLayout {
                        visible: messageList.count === 0
                        anchors.centerIn: parent
                        spacing: 8
                        AppIcon { Layout.alignment: Qt.AlignHCenter; name: "lock"; width: 25; height: 25; color: Theme.textSubtle }
                        Text { text: "Esta conversación empieza aquí"; color: Theme.text; font.pixelSize: 14; font.weight: Font.DemiBold }
                        Text { text: "Los mensajes se cifran antes de salir de tu dispositivo."; color: Theme.textMuted; font.pixelSize: 11 }
                    }
                }

                ColumnLayout {
                    visible: App.contacts.length > 0
                    Layout.fillWidth: true
                    Layout.leftMargin: 22
                    Layout.rightMargin: 22
                    Layout.bottomMargin: 18
                    spacing: 7

                    Repeater {
                        model: App.transfers
                        delegate: SectionCard {
                            required property var modelData
                            Layout.fillWidth: true
                            Layout.preferredHeight: 48
                            color: Theme.positiveSoft
                            RowLayout {
                                anchors.fill: parent; anchors.leftMargin: 12; anchors.rightMargin: 9; spacing: 10
                                AppIcon { name: "file"; width: 17; height: 17; color: Theme.positive }
                                Text { Layout.preferredWidth: 180; text: modelData.fileName; color: Theme.text; elide: Text.ElideMiddle; font.pixelSize: 11 }
                                AppProgressBar { Layout.fillWidth: true; from: 0; to: 1; value: modelData.progress }
                                Text { text: Math.round(modelData.progress * 100) + "%"; color: Theme.textMuted; font.pixelSize: 10 }
                                ActionButton { text: "Cancelar"; compact: true; kind: "ghost"; visible: modelData.cancelable; onClicked: App.cancelTransfer(modelData.id) }
                            }
                        }
                    }

                    SectionCard {
                        visible: window.replyMessageId.length > 0 || window.editMessageId.length > 0
                        Layout.fillWidth: true
                        Layout.preferredHeight: visible ? 44 : 0
                        color: Theme.surfaceHigh
                        RowLayout {
                            anchors.fill: parent; anchors.leftMargin: 12; anchors.rightMargin: 7; spacing: 9
                            AppIcon { name: window.editMessageId.length > 0 ? "file" : "reply"; width: 16; height: 16; color: Theme.accent }
                            ColumnLayout {
                                Layout.fillWidth: true; spacing: 0
                                Text { text: window.editMessageId.length > 0 ? "Editando mensaje" : "Respondiendo"; color: Theme.accent; font.pixelSize: 10; font.weight: Font.DemiBold }
                                Text { Layout.fillWidth: true; text: window.contextBody; color: Theme.textMuted; elide: Text.ElideRight; font.pixelSize: 10 }
                            }
                            IconButton { iconName: "close"; description: "Cancelar"; buttonSize: 30; onClicked: { window.cancelComposerContext(); composer.text = App.draft() } }
                        }
                    }

                    Rectangle {
                        Layout.fillWidth: true
                        Layout.preferredHeight: Math.max(58, composer.contentHeight + 24)
                        radius: Theme.radiusLarge
                        color: Theme.surface
                        border.color: composer.activeFocus ? Theme.accent : Theme.border
                        border.width: composer.activeFocus ? 1.5 : 1
                        RowLayout {
                            anchors.fill: parent; anchors.leftMargin: 8; anchors.rightMargin: 8; spacing: 5
                            IconButton { iconName: "attachment"; description: "Adjuntar archivo cifrado"; onClicked: attachmentDialog.open() }
                            TextArea {
                                id: composer
                                Layout.fillWidth: true
                                Layout.fillHeight: true
                                placeholderText: window.editMessageId.length > 0 ? "Edita el mensaje" : (window.replyMessageId.length > 0 ? "Escribe una respuesta" : "Escribe un mensaje")
                                placeholderTextColor: Theme.textSubtle
                                color: Theme.text
                                selectionColor: Theme.accentStrong
                                wrapMode: TextEdit.Wrap
                                font.pixelSize: 13
                                background: null
                                onTextChanged: if (!window.loadingDraft && window.editMessageId.length === 0) draftTimer.restart()
                                Keys.onPressed: event => {
                                    if ((event.key === Qt.Key_Return || event.key === Qt.Key_Enter) && !(event.modifiers & Qt.ShiftModifier)) {
                                        window.submitComposer(); event.accepted = true
                                    }
                                }
                            }
                            IconButton { iconName: "send"; description: "Enviar mensaje"; active: composer.text.trim().length > 0; enabled: composer.text.trim().length > 0; onClicked: window.submitComposer() }
                        }
                    }
                }

                Timer { id: draftTimer; interval: 450; repeat: false; onTriggered: App.saveDraft(composer.text) }
            }
        }
    }

    AppMenu {
        id: newConversationMenu
        AppMenuItem { text: "Añadir contacto"; onTriggered: inviteDialog.open() }
        AppMenuItem { text: "Crear grupo privado"; enabled: App.directContacts.length > 0; onTriggered: groupDialog.open() }
    }

    AppMenu {
        id: callMenu
        AppMenuItem { text: "Llamar"; onTriggered: App.startCall(true) }
        AppMenuItem { text: "Abrir sala sin hacer sonar"; onTriggered: App.startCall(false) }
    }

    AppDialog {
        id: callParticipantsDialog
        title: "Personas en la llamada"
        width: 480
        contentItem: ColumnLayout {
            spacing: 10
            Text { Layout.fillWidth: true; text: "El volumen sólo cambia lo que tú escuchas."; color: Theme.textMuted; wrapMode: Text.Wrap; font.pixelSize: 11 }
            Repeater {
                model: App.callParticipants
                delegate: SectionCard {
                    required property var modelData
                    Layout.fillWidth: true
                    Layout.preferredHeight: 70
                    RowLayout {
                        anchors.fill: parent; anchors.margins: 12; spacing: 10
                        Avatar { label: modelData.name; size: 36 }
                        Text { Layout.preferredWidth: 110; text: modelData.name; color: Theme.text; elide: Text.ElideRight; font.pixelSize: 12; font.weight: Font.DemiBold }
                        AppSlider { id: participantVolume; Layout.fillWidth: true; from: 0; to: 2; value: modelData.volume; stepSize: 0.05; onMoved: App.setParticipantVolume(modelData.deviceId, value) }
                        Text { Layout.preferredWidth: 40; horizontalAlignment: Text.AlignRight; text: Math.round(participantVolume.value * 100) + "%"; color: participantVolume.value === 0 ? Theme.danger : Theme.textMuted; font.pixelSize: 10 }
                    }
                }
            }
            Text { visible: App.callParticipants.length === 0; Layout.fillWidth: true; text: "Esperando a que alguien se una…"; color: Theme.textMuted; horizontalAlignment: Text.AlignHCenter; font.pixelSize: 12 }
        }
    }

    AppMenu {
        id: contactMenu
        AppMenuItem { text: App.currentConversationPinned ? "Desfijar conversación" : "Fijar conversación"; onTriggered: App.setCurrentConversationPreferences(!App.currentConversationPinned, App.currentConversationArchived, App.currentConversationMuted) }
        AppMenuItem { text: App.currentConversationMuted ? "Activar avisos" : "Silenciar avisos y llamadas"; onTriggered: App.setCurrentConversationPreferences(App.currentConversationPinned, App.currentConversationArchived, !App.currentConversationMuted) }
        AppMenuItem { text: App.currentConversationArchived ? "Sacar del archivo" : "Archivar"; onTriggered: App.setCurrentConversationPreferences(App.currentConversationPinned, !App.currentConversationArchived, App.currentConversationMuted) }
        AppMenuSeparator {}
        AppMenuItem { text: App.currentContactPrivacyHidden ? "Compartir mi presencia" : "Ocultar mi presencia"; onTriggered: App.setCurrentContactPrivacy(!App.currentContactPrivacyHidden) }
        AppMenuItem { text: App.currentContactVerified ? "Revisar verificación" : "Verificar identidad"; onTriggered: verifyContactDialog.open() }
        AppMenuItem { text: App.currentContactBlocked ? "Desbloquear" : "Bloquear"; onTriggered: App.setCurrentContactBlocked(!App.currentContactBlocked) }
        AppMenuSeparator {}
        AppMenuItem { text: "Eliminar contacto"; onTriggered: removeContactDialog.open() }
    }

    AppDialog {
        id: verifyContactDialog
        title: "Verificar identidad"
        width: 500
        contentItem: ColumnLayout {
            spacing: 14
            RowLayout {
                Layout.fillWidth: true
                Rectangle { width: 44; height: 44; radius: 14; color: Theme.positiveSoft; AppIcon { anchors.centerIn: parent; name: "shield"; width: 21; height: 21; color: Theme.positive } }
                Text { Layout.fillWidth: true; text: "Compara esta huella con " + App.conversationName + " por voz o en persona. Los bloques deben ser idénticos."; color: Theme.textMuted; wrapMode: Text.Wrap; font.pixelSize: 12 }
            }
            AppTextArea { Layout.fillWidth: true; Layout.preferredHeight: 100; readOnly: true; selectByMouse: true; text: App.currentContactFingerprint; wrapMode: TextEdit.Wrap }
            RowLayout {
                Layout.fillWidth: true
                Item { Layout.fillWidth: true }
                ActionButton { text: App.currentContactVerified ? "Quitar verificación" : "Coincide, marcar verificado"; iconName: "check"; kind: App.currentContactVerified ? "secondary" : "primary"; onClicked: { App.setCurrentContactVerified(!App.currentContactVerified); verifyContactDialog.close() } }
            }
        }
    }

    AppDialog {
        id: removeContactDialog
        title: "Eliminar contacto"
        width: 440
        contentItem: ColumnLayout {
            spacing: 16
            Text { Layout.fillWidth: true; text: "Se conservará el historial local. Para volver a conectar hará falta una invitación nueva."; color: Theme.textMuted; wrapMode: Text.Wrap; font.pixelSize: 12 }
            RowLayout { Layout.fillWidth: true; Item { Layout.fillWidth: true } ActionButton { text: "Cancelar"; onClicked: removeContactDialog.close() } ActionButton { text: "Eliminar contacto"; iconName: "trash"; kind: "danger"; onClicked: { App.removeCurrentContact(); removeContactDialog.close() } } }
        }
    }

    FileDialog { id: attachmentDialog; title: "Enviar archivo cifrado"; fileMode: FileDialog.OpenFile; onAccepted: App.sendFile(selectedFile) }

    AppDialog {
        id: manageGroupDialog
        title: "Gestionar grupo"
        width: 500
        contentItem: ColumnLayout {
            spacing: 13
            Flow {
                Layout.fillWidth: true; spacing: 7
                ActionButton { text: App.currentConversationPinned ? "Desfijar" : "Fijar"; iconName: "pin"; compact: true; onClicked: App.setCurrentConversationPreferences(!App.currentConversationPinned, App.currentConversationArchived, App.currentConversationMuted) }
                ActionButton { text: App.currentConversationMuted ? "Activar avisos" : "Silenciar"; iconName: "bell-off"; compact: true; onClicked: App.setCurrentConversationPreferences(App.currentConversationPinned, App.currentConversationArchived, !App.currentConversationMuted) }
                ActionButton { text: App.currentConversationArchived ? "Desarchivar" : "Archivar"; iconName: "archive"; compact: true; onClicked: App.setCurrentConversationPreferences(App.currentConversationPinned, !App.currentConversationArchived, App.currentConversationMuted) }
            }
            Text { Layout.fillWidth: true; text: App.currentGroupOwned ? "Eres propietario: puedes gestionar miembros, administradores y propiedad." : (App.currentGroupAdmin ? "Eres administrador: puedes añadir o expulsar miembros normales." : "Sólo los administradores pueden cambiar la membresía."); color: Theme.textMuted; wrapMode: Text.Wrap; font.pixelSize: 11 }
            AppComboBox { id: membershipContact; Layout.fillWidth: true; model: App.directContacts; textRole: "name" }
            RowLayout {
                Layout.fillWidth: true
                ActionButton { enabled: (App.currentGroupOwned || App.currentGroupAdmin) && membershipContact.currentIndex >= 0; text: "Expulsar"; compact: true; kind: "danger"; onClicked: App.removeGroupMember(membershipContact.currentText) }
                ActionButton { enabled: (App.currentGroupOwned || App.currentGroupAdmin) && membershipContact.currentIndex >= 0; text: "Añadir"; compact: true; kind: "primary"; onClicked: App.addGroupMember(membershipContact.currentText) }
                Item { Layout.fillWidth: true }
            }
            RowLayout {
                visible: App.currentGroupOwned
                Layout.fillWidth: true
                ActionButton { text: "Quitar admin"; compact: true; onClicked: App.setGroupAdministrator(membershipContact.currentText, false) }
                ActionButton { text: "Hacer admin"; compact: true; onClicked: App.setGroupAdministrator(membershipContact.currentText, true) }
            }
            ActionButton { visible: App.currentGroupOwned; text: "Transferir propiedad"; compact: true; onClicked: App.transferGroupOwnership(membershipContact.currentText) }
            Rectangle { visible: App.currentGroupOwned; Layout.fillWidth: true; height: 1; color: Theme.border }
            ActionButton { visible: App.currentGroupOwned; text: "Disolver grupo para todos"; iconName: "trash"; kind: "danger"; onClicked: dissolveGroupDialog.open() }
        }
    }

    AppDialog {
        id: dissolveGroupDialog
        title: "Disolver grupo"
        width: 440
        contentItem: ColumnLayout {
            spacing: 16
            Text { Layout.fillWidth: true; text: "El grupo desaparecerá para todos sus miembros. El historial local no se borrará."; color: Theme.textMuted; wrapMode: Text.Wrap; font.pixelSize: 12 }
            RowLayout { Layout.fillWidth: true; Item { Layout.fillWidth: true } ActionButton { text: "Cancelar"; onClicked: dissolveGroupDialog.close() } ActionButton { text: "Disolver grupo"; iconName: "trash"; kind: "danger"; onClicked: { App.dissolveCurrentGroup(); dissolveGroupDialog.close(); manageGroupDialog.close() } } }
        }
    }

    AppDialog {
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
        width: 470
        contentItem: ColumnLayout {
            spacing: 13
            AppTextField { id: groupName; Layout.fillWidth: true; placeholderText: "Nombre del grupo" }
            Text { text: "Participantes"; color: Theme.text; font.pixelSize: 12; font.weight: Font.DemiBold }
            ScrollView {
                Layout.fillWidth: true
                Layout.preferredHeight: Math.min(190, Math.max(54, App.directContacts.length * 44))
                ColumnLayout {
                    width: parent.width
                    Repeater {
                        model: App.directContacts
                        delegate: AppCheckBox {
                            required property var modelData
                            Layout.fillWidth: true
                            text: modelData.name
                            checked: groupDialog.selectedNames.indexOf(modelData.name) >= 0
                            onToggled: groupDialog.toggleMember(modelData.name, checked)
                        }
                    }
                }
            }
            Text { Layout.fillWidth: true; text: "Hasta 16 miembros en el chat y 8 en llamada. Quien entre después no recibe el historial anterior."; color: Theme.textMuted; wrapMode: Text.Wrap; font.pixelSize: 11 }
            RowLayout {
                Layout.fillWidth: true; Item { Layout.fillWidth: true }
                ActionButton { text: "Crear grupo"; iconName: "group"; kind: "primary"; enabled: groupName.text.trim().length > 0 && groupDialog.selectedNames.length > 0; onClicked: { App.createGroup(groupName.text, groupDialog.selectedNames.join(",")); groupName.clear(); groupDialog.selectedNames = []; groupDialog.close() } }
            }
        }
    }

    AppDialog {
        id: inviteDialog
        objectName: "inviteDialog"
        title: "Añadir contacto"
        width: 510
        onOpened: App.createInvite()
        contentItem: ColumnLayout {
            spacing: 14
            TabBar {
                id: inviteMode
                Layout.fillWidth: true
                background: Rectangle { color: Theme.canvas; radius: Theme.radius; border.color: Theme.border }
                TabButton { text: "Invitar"; contentItem: Text { text: parent.text; color: parent.checked ? Theme.text : Theme.textMuted; horizontalAlignment: Text.AlignHCenter; verticalAlignment: Text.AlignVCenter; font.pixelSize: 12 } background: Rectangle { anchors.margins: 4; color: parent.checked ? Theme.surfaceHigh : "transparent"; radius: Theme.radiusSmall } }
                TabButton { text: "Aceptar invitación"; contentItem: Text { text: parent.text; color: parent.checked ? Theme.text : Theme.textMuted; horizontalAlignment: Text.AlignHCenter; verticalAlignment: Text.AlignVCenter; font.pixelSize: 12 } background: Rectangle { anchors.margins: 4; color: parent.checked ? Theme.surfaceHigh : "transparent"; radius: Theme.radiusSmall } }
            }
            StackLayout {
                Layout.fillWidth: true
                currentIndex: inviteMode.currentIndex
                ColumnLayout {
                    spacing: 12
                    Text { Layout.fillWidth: true; text: "Este enlace caduca y sólo puede usarse una vez. Compártelo por un canal de confianza."; color: Theme.textMuted; wrapMode: Text.Wrap; font.pixelSize: 11 }
                    Text { visible: App.inviteLink.length === 0; Layout.fillWidth: true; text: "Creando invitación cifrada…"; color: Theme.textMuted; horizontalAlignment: Text.AlignHCenter; font.pixelSize: 11 }
                    Rectangle { visible: App.inviteQr.length > 0; Layout.alignment: Qt.AlignHCenter; Layout.preferredWidth: 190; Layout.preferredHeight: 190; radius: 14; color: "white"; Image { anchors.fill: parent; anchors.margins: 10; source: App.inviteQr; fillMode: Image.PreserveAspectFit } }
                    AppTextField { Layout.fillWidth: true; readOnly: true; text: App.inviteLink }
                    ActionButton { Layout.alignment: Qt.AlignRight; text: "Copiar enlace"; iconName: "copy"; kind: "primary"; enabled: App.inviteLink.length > 0; onClicked: App.copyInvite() }
                }
                ColumnLayout {
                    spacing: 12
                    Text { Layout.fillWidth: true; text: "Pega el enlace completo que te ha enviado la otra persona."; color: Theme.textMuted; wrapMode: Text.Wrap; font.pixelSize: 11 }
                    AppTextArea { id: incomingInvite; Layout.fillWidth: true; Layout.preferredHeight: 110; placeholderText: "pptalk://contact/v1#..."; wrapMode: TextEdit.WrapAnywhere }
                    Item { Layout.fillHeight: true }
                    ActionButton { Layout.alignment: Qt.AlignRight; text: "Revisar invitación"; iconName: "shield"; kind: "primary"; enabled: incomingInvite.text.trim().length > 0; onClicked: { App.acceptInvite(incomingInvite.text); incomingInvite.clear() } }
                }
            }
        }
    }

    AppDialog {
        id: invitePreviewDialog
        title: "Confirmar contacto"
        width: 460
        contentItem: ColumnLayout {
            spacing: 14
            RowLayout { Avatar { label: App.invitePreviewName; size: 46 } ColumnLayout { Text { text: App.invitePreviewName; color: Theme.text; font.pixelSize: 17; font.weight: Font.DemiBold } Text { text: "Caduca: " + App.invitePreviewExpiry; color: Theme.textMuted; font.pixelSize: 10 } } }
            Text { Layout.fillWidth: true; text: "Acepta sólo si recibiste este enlace por un canal de confianza. Después podrás comparar la huella de identidad."; color: Theme.textMuted; wrapMode: Text.Wrap; font.pixelSize: 11 }
            RowLayout { Layout.fillWidth: true; Item { Layout.fillWidth: true } ActionButton { text: "Cancelar"; onClicked: invitePreviewDialog.close() } ActionButton { text: "Aceptar contacto"; iconName: "check"; kind: "primary"; onClicked: { App.confirmInvite(); invitePreviewDialog.close(); inviteDialog.close() } } }
        }
    }

    SettingsDrawer { id: settingsDrawer; objectName: "settingsDrawer" }

    Onboarding { anchors.fill: parent; visible: App.onboardingRequired; z: 1000 }
}
