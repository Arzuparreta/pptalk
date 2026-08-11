import QtQuick
import QtQuick.Controls
import QtQuick.Dialogs
import QtQuick.Layouts

Rectangle {
    id: root
    color: Theme.canvas

    FileDialog {
        id: restoreFile
        title: "Restaurar identidad"
        fileMode: FileDialog.OpenFile
        nameFilters: ["Copias de pptalk (*.pptalk-backup)", "Todos los archivos (*)"]
    }

    Rectangle {
        anchors.fill: parent
        gradient: Gradient {
            GradientStop { position: 0; color: "#11182A" }
            GradientStop { position: 0.55; color: Theme.canvas }
            GradientStop { position: 1; color: "#0B1218" }
        }
    }

    RowLayout {
        anchors.centerIn: parent
        width: Math.min(930, parent.width - 72)
        spacing: 54

        ColumnLayout {
            Layout.preferredWidth: 350
            spacing: 18
            Rectangle {
                width: 58; height: 58; radius: 18
                color: Theme.accentStrong
                Text { anchors.centerIn: parent; text: "p"; color: "white"; font.pixelSize: 32; font.weight: Font.Bold }
            }
            Text {
                Layout.fillWidth: true
                text: "Habla con tu gente.\nNada más en medio."
                color: Theme.text
                font.pixelSize: 30
                font.weight: Font.DemiBold
                lineHeight: 1.08
            }
            Text {
                Layout.fillWidth: true
                text: "Mensajes, archivos y llamadas privadas desde una identidad que vive en tus dispositivos. Sin cuenta central."
                color: Theme.textMuted
                wrapMode: Text.Wrap
                font.pixelSize: 14
                lineHeight: 1.35
            }
            Row { spacing: 8; AppIcon { name: "shield"; color: Theme.positive; width: 17; height: 17 } Text { text: "Cifrado de extremo a extremo"; color: Theme.textMuted; font.pixelSize: 12 } }
            Row { spacing: 8; AppIcon { name: "device"; color: Theme.accent; width: 17; height: 17 } Text { text: "Identidad local y portable"; color: Theme.textMuted; font.pixelSize: 12 } }
        }

        SectionCard {
            Layout.preferredWidth: 500
            Layout.preferredHeight: 470
            ColumnLayout {
                anchors.fill: parent
                anchors.margins: 28
                spacing: 18
                Text { text: "Configura este dispositivo"; color: Theme.text; font.pixelSize: 20; font.weight: Font.DemiBold }
                Text { text: "Elige cómo quieres empezar."; color: Theme.textMuted; font.pixelSize: 12 }

                TabBar {
                    id: mode
                    Layout.fillWidth: true
                    background: Rectangle { color: Theme.canvas; radius: Theme.radius; border.color: Theme.border }
                    Repeater {
                        model: ["Nueva identidad", "Vincular", "Restaurar"]
                        TabButton {
                            required property string modelData
                            text: modelData
                            contentItem: Text { text: parent.text; color: parent.checked ? Theme.text : Theme.textMuted; horizontalAlignment: Text.AlignHCenter; verticalAlignment: Text.AlignVCenter; font.pixelSize: 12; font.weight: parent.checked ? Font.DemiBold : Font.Normal }
                            background: Rectangle { anchors.margins: 4; color: parent.checked ? Theme.surfaceHigh : "transparent"; radius: Theme.radiusSmall }
                        }
                    }
                }

                StackLayout {
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    currentIndex: mode.currentIndex

                    ColumnLayout {
                        spacing: 12
                        Item { Layout.preferredHeight: 6 }
                        Text { text: "Crea una identidad sólo para ti"; color: Theme.text; font.pixelSize: 14; font.weight: Font.DemiBold }
                        Text { Layout.fillWidth: true; text: "Tus amigos verán este nombre. Podrás cambiarlo más tarde."; color: Theme.textMuted; wrapMode: Text.Wrap; font.pixelSize: 12 }
                        AppTextField { id: newName; Layout.fillWidth: true; placeholderText: "Tu nombre" }
                        Item { Layout.fillHeight: true }
                        ActionButton { Layout.fillWidth: true; text: "Crear identidad local"; iconName: "shield"; kind: "primary"; enabled: newName.text.trim().length > 0; onClicked: App.initializeProfile(newName.text) }
                    }

                    ColumnLayout {
                        spacing: 12
                        Item { Layout.preferredHeight: 6 }
                        Text { text: "Añade este equipo a tu identidad"; color: Theme.text; font.pixelSize: 14; font.weight: Font.DemiBold }
                        Text { Layout.fillWidth: true; text: "Genera el enlace desde Ajustes en uno de tus dispositivos autorizados."; color: Theme.textMuted; wrapMode: Text.Wrap; font.pixelSize: 12 }
                        AppTextArea { id: deviceLink; Layout.fillWidth: true; Layout.preferredHeight: 110; placeholderText: "Pega el enlace pptalk://device/…"; text: App.onboardingLink; wrapMode: TextEdit.WrapAnywhere }
                        Item { Layout.fillHeight: true }
                        ActionButton { Layout.fillWidth: true; text: "Vincular este equipo"; iconName: "device"; kind: "primary"; enabled: deviceLink.text.trim().length > 0; onClicked: App.importDeviceLink(deviceLink.text) }
                    }

                    ColumnLayout {
                        spacing: 12
                        Item { Layout.preferredHeight: 6 }
                        Text { text: "Recupera una copia cifrada"; color: Theme.text; font.pixelSize: 14; font.weight: Font.DemiBold }
                        RowLayout {
                            Layout.fillWidth: true
                            AppTextField { Layout.fillWidth: true; readOnly: true; text: restoreFile.selectedFile; placeholderText: "Copia .pptalk-backup" }
                            ActionButton { text: "Elegir"; iconName: "file"; onClicked: restoreFile.open() }
                        }
                        AppTextField { id: restorePassphrase; Layout.fillWidth: true; echoMode: TextInput.Password; placeholderText: "Frase de la copia (mínimo 10 caracteres)" }
                        Item { Layout.fillHeight: true }
                        ActionButton { Layout.fillWidth: true; text: "Restaurar identidad"; iconName: "lock"; kind: "primary"; enabled: restoreFile.selectedFile.toString().length > 0 && restorePassphrase.text.length >= 10; onClicked: App.restoreIdentityBackup(restoreFile.selectedFile, restorePassphrase.text) }
                    }
                }

                Text {
                    visible: App.lastError.length > 0
                    Layout.fillWidth: true
                    text: App.lastError
                    color: Theme.danger
                    horizontalAlignment: Text.AlignHCenter
                    wrapMode: Text.Wrap
                    font.pixelSize: 11
                }
            }
        }
    }
}
