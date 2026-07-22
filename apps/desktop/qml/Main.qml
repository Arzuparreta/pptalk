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
                }

                Text {
                    text: "CONVERSACIONES"
                    color: "#6F697A"
                    font.pixelSize: 10
                    font.letterSpacing: 1.2
                    Layout.topMargin: 6
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
                        Avatar { x: 11; anchors.verticalCenter: parent.verticalCenter; label: modelData.name; accent: modelData.accent; size: 44 }
                        Column {
                            x: 67; anchors.verticalCenter: parent.verticalCenter; spacing: 5
                            Text { text: modelData.name; color: window.text; font.pixelSize: 14; font.weight: Font.Medium }
                            Text { text: modelData.summary; color: window.muted; font.pixelSize: 11; elide: Text.ElideRight; width: 174 }
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
                    Avatar { label: "Tú"; accent: "#77D8B1"; size: 38 }
                    ColumnLayout {
                        spacing: 1
                        Text { text: "Tú"; color: window.text; font.pixelSize: 13; font.weight: Font.Medium }
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
                        Avatar { label: App.conversationName; accent: "#8B7CFF"; size: 40 }
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
                    }
                }

                Rectangle {
                    visible: App.lastError.length > 0
                    Layout.fillWidth: true
                    Layout.preferredHeight: errorText.implicitHeight + 22
                    color: "#3A2028"
                    border.color: "#6F3542"
                    Text {
                        id: errorText
                        anchors.fill: parent
                        anchors.margins: 11
                        text: App.lastError
                        color: "#FFD4DC"
                        wrapMode: Text.Wrap
                        font.pixelSize: 11
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
                            Text { text: "Llamada activa"; color: window.text; font.pixelSize: 14; font.weight: Font.DemiBold }
                            Text { text: "P2P mesh · cifrado extremo a extremo"; color: window.muted; font.pixelSize: 11 }
                        }
                        Item { Layout.fillWidth: true }
                        IconButton { glyph: App.microphoneEnabled ? "◉" : "×"; active: App.microphoneEnabled; onClicked: App.toggleMicrophone() }
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
                    }
                    onCountChanged: positionViewAtEnd()
                }

                Rectangle {
                    Layout.fillWidth: true
                    Layout.leftMargin: 20
                    Layout.rightMargin: 20
                    Layout.bottomMargin: 18
                    Layout.preferredHeight: Math.max(58, composer.contentHeight + 25)
                    radius: 17
                    color: "#211E29"
                    border.color: composer.activeFocus ? "#665B9A" : window.line
                    RowLayout {
                        anchors.fill: parent; anchors.leftMargin: 8; anchors.rightMargin: 8; spacing: 5
                        IconButton { glyph: "+"; onClicked: attachmentDialog.open() }
                        TextArea {
                            id: composer
                            Layout.fillWidth: true
                            placeholderText: "Escribe un mensaje"
                            placeholderTextColor: "#746F80"
                            color: window.text
                            wrapMode: TextEdit.Wrap
                            background: null
                            Keys.onPressed: event => {
                                if ((event.key === Qt.Key_Return || event.key === Qt.Key_Enter) && !(event.modifiers & Qt.ShiftModifier)) {
                                    App.sendMessage(text); clear(); event.accepted = true
                                }
                            }
                        }
                        IconButton {
                            glyph: "➤"; active: composer.text.trim().length > 0
                            onClicked: { App.sendMessage(composer.text); composer.clear() }
                        }
                    }
                }
            }
        }
    }

    Menu {
        id: callMenu
        MenuItem { text: "Abrir sala sin llamar"; onTriggered: App.startCall(false) }
        MenuItem { text: App.conversationIsGroup ? "Llamar al grupo" : "Llamar"; onTriggered: App.startCall(true) }
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
            Text { text: "Sólo el creador puede aplicar cambios de época MLS."; color: window.muted; wrapMode: Text.Wrap; Layout.fillWidth: true; font.pixelSize: 11 }
            TextField { id: membershipContact; Layout.fillWidth: true; placeholderText: "Nombre exacto del contacto" }
            RowLayout {
                Layout.alignment: Qt.AlignRight
                Button { text: "Expulsar"; onClicked: { App.removeGroupMember(membershipContact.text); manageGroupDialog.close() } }
                Button { text: "Añadir"; onClicked: { App.addGroupMember(membershipContact.text); manageGroupDialog.close() } }
            }
        }
    }

    Dialog {
        id: settingsDialog
        title: "Red opcional"
        modal: true
        anchors.centerIn: parent
        width: 450
        background: Rectangle { color: "#211E29"; radius: 18; border.color: "#403A4B" }
        contentItem: ColumnLayout {
            spacing: 12
            Text { text: "Nodo de buzón cifrado"; color: window.text; font.pixelSize: 13; font.weight: Font.DemiBold }
            Text { text: "Déjalo vacío para P2P puro. El nodo sólo almacena sobres opacos cuando estás desconectado."; color: window.muted; wrapMode: Text.Wrap; Layout.fillWidth: true; font.pixelSize: 11 }
            TextField { id: mailboxUrl; Layout.fillWidth: true; placeholderText: "https://tu-nodo.example" }
            Button { text: "Guardar"; Layout.alignment: Qt.AlignRight; onClicked: { App.configureMailbox(mailboxUrl.text); settingsDialog.close() } }
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
        }
    }

    Dialog {
        id: groupDialog
        title: "Nuevo grupo privado"
        modal: true
        anchors.centerIn: parent
        width: 430
        background: Rectangle { color: "#211E29"; radius: 18; border.color: "#403A4B" }
        contentItem: ColumnLayout {
            spacing: 12
            TextField { id: groupName; Layout.fillWidth: true; placeholderText: "Nombre del grupo" }
            TextField { id: groupMembers; Layout.fillWidth: true; placeholderText: "Contactos, separados por comas" }
            Text { text: "Sólo el creador administra la membresía. Los nuevos miembros no reciben el historial anterior."; color: window.muted; wrapMode: Text.Wrap; Layout.fillWidth: true; font.pixelSize: 11 }
            Button {
                text: "Crear con MLS"
                Layout.alignment: Qt.AlignRight
                onClicked: { App.createGroup(groupName.text, groupMembers.text); groupDialog.close() }
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
                    text: "Aceptar"
                    enabled: incomingInvite.text.trim().length > 0
                    onClicked: { App.acceptInvite(incomingInvite.text); incomingInvite.clear(); inviteDialog.close() }
                }
                Item { Layout.fillWidth: true }
                Button { text: "Copiar enlace"; onClicked: App.copyInvite(); background: Rectangle { color: window.accent; radius: 11 } }
            }
        }
    }
}
