import QtQuick
import QtQuick.Controls
import QtQuick.Dialogs
import QtQuick.Layouts

Drawer {
    id: root
    edge: Qt.RightEdge
    modal: true
    dim: true
    closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside
    width: Math.min(590, parent ? parent.width * 0.62 : 590)
    height: parent ? parent.height : 760
    padding: 0
    Overlay.modal: Rectangle { color: "#8006090E" }
    background: Rectangle { color: Theme.sidebar; border.color: Theme.border }

    function mediaChoices(kind) {
        const values = [{ "id": "", "label": "Predeterminado del sistema" }]
        for (let i = 0; i < App.mediaDevices.length; ++i) {
            if (App.mediaDevices[i].kind === kind) values.push(App.mediaDevices[i])
        }
        return values
    }
    function mediaChoiceIndex(kind, choices) {
        const selected = App.selectedMediaDevice(kind)
        for (let i = 0; i < choices.length; ++i) if (choices[i].id === selected) return i
        return 0
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

    ColumnLayout {
        anchors.fill: parent
        spacing: 0

        RowLayout {
            Layout.fillWidth: true
            Layout.preferredHeight: 70
            Layout.leftMargin: 24
            Layout.rightMargin: 16
            Text { text: "Ajustes"; color: Theme.text; font.pixelSize: 21; font.weight: Font.DemiBold }
            Item { Layout.fillWidth: true }
            IconButton { iconName: "close"; description: "Cerrar ajustes"; onClicked: root.close() }
        }

        TabBar {
            id: sections
            Layout.fillWidth: true
            Layout.leftMargin: 20
            Layout.rightMargin: 20
            Layout.bottomMargin: 14
            background: Rectangle { color: Theme.canvas; radius: Theme.radius; border.color: Theme.border }
            Repeater {
                model: ["General", "Audio", "Equipos", "Buzón"]
                TabButton {
                    required property string modelData
                    text: modelData
                    implicitHeight: 42
                    contentItem: Text { text: parent.text; color: parent.checked ? Theme.text : Theme.textMuted; horizontalAlignment: Text.AlignHCenter; verticalAlignment: Text.AlignVCenter; font.pixelSize: 11; font.weight: parent.checked ? Font.DemiBold : Font.Normal }
                    background: Rectangle { anchors.margins: 4; color: parent.checked ? Theme.surfaceHigh : "transparent"; radius: Theme.radiusSmall }
                }
            }
        }

        StackLayout {
            Layout.fillWidth: true
            Layout.fillHeight: true
            currentIndex: sections.currentIndex

            ScrollView {
                clip: true
                contentWidth: availableWidth
                ColumnLayout {
                    x: 20
                    width: parent.width - 40
                    spacing: 14

                    SectionCard {
                        Layout.fillWidth: true
                        Layout.preferredHeight: profileContent.implicitHeight + 36
                        ColumnLayout {
                            id: profileContent
                            anchors.fill: parent; anchors.margins: 18; spacing: 12
                            Text { text: "Perfil"; color: Theme.text; font.pixelSize: 14; font.weight: Font.DemiBold }
                            RowLayout {
                                Layout.fillWidth: true
                                Avatar { label: profileName.text; source: App.profileAvatar; size: 48 }
                                AppTextField { id: profileName; Layout.fillWidth: true; text: App.profileName; placeholderText: "Tu nombre" }
                                ActionButton { text: "Guardar"; compact: true; kind: "primary"; enabled: profileName.text.trim().length > 0 && profileName.text.trim() !== App.profileName; onClicked: App.updateProfile(profileName.text, "") }
                            }
                            RowLayout {
                                ActionButton { text: "Cambiar avatar"; compact: true; iconName: "file"; onClicked: avatarDialog.open() }
                                ActionButton { text: "Quitar"; compact: true; kind: "ghost"; enabled: App.profileAvatar.length > 0; onClicked: App.clearProfileAvatar() }
                            }
                        }
                    }

                    SectionCard {
                        Layout.fillWidth: true
                        Layout.preferredHeight: generalContent.implicitHeight + 36
                        ColumnLayout {
                            id: generalContent
                            anchors.fill: parent; anchors.margins: 18; spacing: 8
                            Text { text: "Aplicación"; color: Theme.text; font.pixelSize: 14; font.weight: Font.DemiBold }
                            AppSwitch { text: "No molestar"; checked: App.doNotDisturb; onToggled: App.doNotDisturb = checked }
                            Text { Layout.fillWidth: true; text: "Los mensajes siguen llegando, pero pptalk no reproduce avisos ni timbres."; color: Theme.textMuted; wrapMode: Text.Wrap; font.pixelSize: 11 }
                            Loader { active: App.platformSupportsAutostart; visible: active; source: active ? "DesktopAutostartSetting.qml" : "" }
                        }
                    }

                    SectionCard {
                        visible: App.updateAvailable
                        Layout.fillWidth: true
                        Layout.preferredHeight: 76
                        RowLayout {
                            anchors.fill: parent; anchors.margins: 16
                            AppIcon { name: "download"; color: Theme.accent; width: 20; height: 20 }
                            Text { Layout.fillWidth: true; text: "Nueva versión " + App.updateVersion; color: Theme.text; font.pixelSize: 12 }
                            ActionButton { text: "Descargar"; compact: true; kind: "primary"; onClicked: App.downloadUpdate() }
                        }
                    }
                }
            }

            ScrollView {
                clip: true
                contentWidth: availableWidth
                ColumnLayout {
                    x: 20
                    width: parent.width - 40
                    spacing: 14

                    SectionCard {
                        Layout.fillWidth: true
                        Layout.preferredHeight: voiceContent.implicitHeight + 36
                        ColumnLayout {
                            id: voiceContent
                            anchors.fill: parent; anchors.margins: 18; spacing: 10
                            Text { text: "Cómo hablas"; color: Theme.text; font.pixelSize: 14; font.weight: Font.DemiBold }
                            AppComboBox { Layout.fillWidth: true; model: ["Micrófono abierto", "Pulsar para hablar"]; currentIndex: App.voiceMode === "push_to_talk" ? 1 : 0; onActivated: App.voiceMode = currentIndex === 1 ? "push_to_talk" : "open" }
                            AppComboBox {
                                visible: App.voiceMode === "push_to_talk"
                                Layout.fillWidth: true
                                model: [{ "label": "Ctrl + Espacio", "value": "Ctrl+Space" }, { "label": "Alt + Espacio", "value": "Alt+Space" }, { "label": "F8", "value": "F8" }]
                                textRole: "label"
                                currentIndex: App.pushToTalkShortcut === "Alt+Space" ? 1 : (App.pushToTalkShortcut === "F8" ? 2 : 0)
                                onActivated: App.pushToTalkShortcut = model[currentIndex].value
                            }
                            Text { visible: App.voiceMode === "push_to_talk"; Layout.fillWidth: true; text: "Mantén el atajo mientras pptalk tenga el foco."; color: Theme.textMuted; font.pixelSize: 11 }
                        }
                    }

                    SectionCard {
                        Layout.fillWidth: true
                        Layout.preferredHeight: devicesContent.implicitHeight + 36
                        ColumnLayout {
                            id: devicesContent
                            anchors.fill: parent; anchors.margins: 18; spacing: 9
                            Text { text: "Dispositivos multimedia"; color: Theme.text; font.pixelSize: 14; font.weight: Font.DemiBold }
                            Text { text: "Micrófono"; color: Theme.textMuted; font.pixelSize: 11 }
                            AppComboBox { Layout.fillWidth: true; property var choices: root.mediaChoices("audio_input"); model: choices; textRole: "label"; currentIndex: root.mediaChoiceIndex("audio_input", choices); onActivated: App.selectMediaDevice("audio_input", choices[currentIndex].id) }
                            RowLayout {
                                Layout.fillWidth: true
                                ActionButton { text: "Probar micrófono"; compact: true; iconName: "mic"; onClicked: App.testMicrophone() }
                                Text { Layout.fillWidth: true; text: App.microphoneTestStatus; color: App.microphoneTestStatus === "Micrófono listo." ? Theme.positive : Theme.textMuted; wrapMode: Text.Wrap; font.pixelSize: 10 }
                            }
                            Text { text: "Altavoces o auriculares"; color: Theme.textMuted; font.pixelSize: 11 }
                            AppComboBox { Layout.fillWidth: true; property var choices: root.mediaChoices("audio_output"); model: choices; textRole: "label"; currentIndex: root.mediaChoiceIndex("audio_output", choices); onActivated: App.selectMediaDevice("audio_output", choices[currentIndex].id) }
                            Text { text: "Cámara"; color: Theme.textMuted; font.pixelSize: 11 }
                            AppComboBox { Layout.fillWidth: true; property var choices: root.mediaChoices("camera"); model: choices; textRole: "label"; currentIndex: root.mediaChoiceIndex("camera", choices); onActivated: App.selectMediaDevice("camera", choices[currentIndex].id) }
                        }
                    }

                    SectionCard {
                        Layout.fillWidth: true
                        Layout.preferredHeight: qualityContent.implicitHeight + 36
                        ColumnLayout {
                            id: qualityContent
                            anchors.fill: parent; anchors.margins: 18; spacing: 9
                            Text { text: "Cámara y pantalla"; color: Theme.text; font.pixelSize: 14; font.weight: Font.DemiBold }
                            AppComboBox { id: videoQuality; Layout.fillWidth: true; model: ["Automática", "720p · 30 fps", "1080p · 30 fps", "1440p · 60 fps", "4K · 60 fps"]; currentIndex: App.videoQualityPreset; onActivated: App.configureVideoQuality(currentIndex) }
                            Text { Layout.fillWidth: true; text: videoQuality.currentIndex === 0 ? "La calidad se adapta a la red." : "El modo manual avisa si el equipo no puede cumplirlo; no baja la calidad en silencio."; color: Theme.textMuted; wrapMode: Text.Wrap; font.pixelSize: 11 }
                        }
                    }
                }
            }

            ScrollView {
                clip: true
                contentWidth: availableWidth
                ColumnLayout {
                    x: 20
                    width: parent.width - 40
                    spacing: 14

                    SectionCard {
                        Layout.fillWidth: true
                        Layout.preferredHeight: linkContent.implicitHeight + 36
                        ColumnLayout {
                            id: linkContent
                            anchors.fill: parent; anchors.margins: 18; spacing: 10
                            Text { text: "Vincular otro dispositivo"; color: Theme.text; font.pixelSize: 14; font.weight: Font.DemiBold }
                            Text { Layout.fillWidth: true; text: "El enlace caduca a los 10 minutos."; color: Theme.textMuted; wrapMode: Text.Wrap; font.pixelSize: 11 }
                            AppTextField { id: deviceLabel; Layout.fillWidth: true; placeholderText: "Nombre, por ejemplo Portátil" }
                            RowLayout {
                                ActionButton { text: "Generar enlace"; compact: true; iconName: "device"; kind: "primary"; onClicked: App.createDeviceLink(deviceLabel.text) }
                                ActionButton { text: "Copiar"; compact: true; iconName: "copy"; enabled: App.deviceLink.length > 0; onClicked: App.copyDeviceLink() }
                            }
                            AppTextArea { visible: App.deviceLink.length > 0; Layout.fillWidth: true; Layout.preferredHeight: 82; readOnly: true; text: App.deviceLink; wrapMode: TextEdit.WrapAnywhere }
                            Text { text: "Dispositivos autorizados"; color: Theme.text; font.pixelSize: 12; font.weight: Font.DemiBold }
                            Repeater {
                                model: App.devices
                                delegate: RowLayout {
                                    required property var modelData
                                    Layout.fillWidth: true
                                    AppIcon { name: "device"; width: 16; height: 16; color: modelData.active ? Theme.positive : Theme.textSubtle }
                                    Text { Layout.fillWidth: true; text: modelData.label + (modelData.current ? " · este dispositivo" : (modelData.active ? " · activo" : " · revocado")); color: modelData.active ? Theme.text : Theme.textSubtle; font.pixelSize: 11 }
                                    ActionButton { visible: modelData.active && !modelData.current; text: "Revocar"; compact: true; kind: "danger"; onClicked: App.revokeDevice(modelData.id) }
                                }
                            }
                        }
                    }

                    SectionCard {
                        Layout.fillWidth: true
                        Layout.preferredHeight: securityContent.implicitHeight + 36
                        ColumnLayout {
                            id: securityContent
                            anchors.fill: parent; anchors.margins: 18; spacing: 10
                            Text { text: "Protección local"; color: Theme.text; font.pixelSize: 14; font.weight: Font.DemiBold }
                            Text { Layout.fillWidth: true; text: App.secureStorageEnabled ? "La clave del historial está protegida por el sistema." : "Mueve la clave del historial al almacén seguro sin cambiar tu identidad."; color: App.secureStorageEnabled ? Theme.positive : Theme.textMuted; wrapMode: Text.Wrap; font.pixelSize: 11 }
                            ActionButton { text: App.secureStorageEnabled ? "Protección activada" : "Proteger con el sistema"; iconName: "lock"; compact: true; enabled: !App.secureStorageEnabled; onClicked: App.protectLocalSecrets() }
                        }
                    }

                    SectionCard {
                        Layout.fillWidth: true
                        Layout.preferredHeight: backupContent.implicitHeight + 36
                        ColumnLayout {
                            id: backupContent
                            anchors.fill: parent; anchors.margins: 18; spacing: 10
                            Text { text: "Copia cifrada de identidad"; color: Theme.text; font.pixelSize: 14; font.weight: Font.DemiBold }
                            Text { Layout.fillWidth: true; text: "Incluye identidad, contactos y grupos. El historial y los adjuntos se quedan en este equipo."; color: Theme.textMuted; wrapMode: Text.Wrap; font.pixelSize: 11 }
                            AppTextField { id: backupPassphrase; Layout.fillWidth: true; echoMode: TextInput.Password; placeholderText: "Frase para la copia (mínimo 10 caracteres)" }
                            ActionButton { text: "Guardar copia cifrada"; iconName: "download"; compact: true; kind: "primary"; enabled: backupPassphrase.text.length >= 10; onClicked: backupExportDialog.open() }
                            Text { visible: App.backupStatus.length > 0; Layout.fillWidth: true; text: App.backupStatus; color: Theme.positive; wrapMode: Text.Wrap; font.pixelSize: 10 }
                        }
                    }
                }
            }

            ScrollView {
                clip: true
                contentWidth: availableWidth
                ColumnLayout {
                    x: 20
                    width: parent.width - 40
                    spacing: 14
                    SectionCard {
                        Layout.fillWidth: true
                        Layout.preferredHeight: mailboxContent.implicitHeight + 36
                        ColumnLayout {
                            id: mailboxContent
                            anchors.fill: parent; anchors.margins: 18; spacing: 11
                            Row { spacing: 9; AppIcon { name: "inbox"; width: 20; height: 20; color: Theme.accent } Text { text: "Mensajes sin conexión"; color: Theme.text; font.pixelSize: 14; font.weight: Font.DemiBold } }
                            Text { Layout.fillWidth: true; text: "Sin buzón, los mensajes esperan cifrados en tu equipo hasta que ambos estéis conectados. Un buzón opcional los guarda ya cifrados mientras estás fuera."; color: Theme.textMuted; wrapMode: Text.Wrap; font.pixelSize: 11; lineHeight: 1.25 }
                            AppTextField { id: mailboxField; Layout.fillWidth: true; text: App.mailboxUrl; placeholderText: "https://buzon.example" }
                            RowLayout {
                                ActionButton { text: "Guardar buzón"; compact: true; kind: "primary"; enabled: mailboxField.text.trim().length > 0 && mailboxField.text.trim().replace(/\/+$/, "") !== App.mailboxUrl.replace(/\/+$/, ""); onClicked: App.setMailbox(mailboxField.text) }
                                ActionButton { text: "Quitar"; compact: true; kind: "danger"; enabled: App.mailboxUrl.length > 0; onClicked: { mailboxField.text = ""; App.clearMailbox() } }
                            }
                            Text { visible: App.mailboxStatus.length > 0; Layout.fillWidth: true; text: App.mailboxStatus; color: App.mailboxUrl.length > 0 ? Theme.positive : Theme.textMuted; wrapMode: Text.Wrap; font.pixelSize: 11 }
                            Rectangle { Layout.fillWidth: true; height: 1; color: Theme.border }
                            Row { spacing: 8; AppIcon { name: "shield"; width: 16; height: 16; color: Theme.positive } Text { text: "El buzón nunca recibe contenido sin cifrar."; color: Theme.textMuted; font.pixelSize: 11 } }
                        }
                    }
                }
            }
        }
    }
}
