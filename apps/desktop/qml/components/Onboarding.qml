import QtQuick
import QtQuick.Controls
import QtQuick.Dialogs
import QtQuick.Layouts

Rectangle {
    id: root
    color: "#111016"

    property color textColor: "#F4F2F7"
    property color mutedColor: "#908B9D"
    property color accentColor: "#8173F2"

    FileDialog {
        id: restoreFile
        title: "Restaurar identidad"
        fileMode: FileDialog.OpenFile
        nameFilters: ["Copias de pptalk (*.pptalk-backup)", "Todos los archivos (*)"]
    }

    ColumnLayout {
        anchors.centerIn: parent
        width: Math.min(620, parent.width - 48)
        spacing: 20

        Rectangle {
            Layout.alignment: Qt.AlignHCenter
            width: 64; height: 64; radius: 20
            gradient: Gradient {
                GradientStop { position: 0; color: "#9D8CFF" }
                GradientStop { position: 1; color: "#5B4ACB" }
            }
            Text {
                anchors.centerIn: parent
                text: "p"
                color: "white"
                font.pixelSize: 38
                font.weight: Font.Bold
            }
        }
        Text {
            Layout.alignment: Qt.AlignHCenter
            text: "Tu espacio privado para hablar y jugar"
            color: root.textColor
            font.pixelSize: 24
            font.weight: Font.DemiBold
        }
        Text {
            Layout.fillWidth: true
            text: "No necesitas registrarte. Puedes crear una identidad en este equipo, vincular una que ya tengas o restaurar una copia cifrada."
            color: root.mutedColor
            horizontalAlignment: Text.AlignHCenter
            wrapMode: Text.Wrap
            font.pixelSize: 13
        }

        TabBar {
            id: mode
            Layout.fillWidth: true
            TabButton { text: "Empezar" }
            TabButton { text: "Vincular equipo" }
            TabButton { text: "Restaurar copia" }
        }

        StackLayout {
            Layout.fillWidth: true
            currentIndex: mode.currentIndex

            ColumnLayout {
                spacing: 12
                TextField {
                    id: newName
                    Layout.fillWidth: true
                    placeholderText: "Cómo quieres que te vean tus amigos"
                    text: ""
                }
                Button {
                    Layout.fillWidth: true
                    text: "Crear identidad local"
                    enabled: newName.text.trim().length > 0
                    onClicked: App.initializeProfile(newName.text)
                }
            }

            ColumnLayout {
                spacing: 12
                TextArea {
                    id: deviceLink
                    Layout.fillWidth: true
                    Layout.preferredHeight: 100
                    placeholderText: "Pega el enlace pptalk://device/…"
                    text: App.onboardingLink
                    wrapMode: TextEdit.WrapAnywhere
                }
                Button {
                    Layout.fillWidth: true
                    text: "Vincular este equipo"
                    enabled: deviceLink.text.trim().length > 0
                    onClicked: App.importDeviceLink(deviceLink.text)
                }
            }

            ColumnLayout {
                spacing: 12
                RowLayout {
                    Layout.fillWidth: true
                    TextField {
                        Layout.fillWidth: true
                        readOnly: true
                        text: restoreFile.selectedFile
                        placeholderText: "Selecciona una copia .pptalk-backup"
                    }
                    Button { text: "Elegir"; onClicked: restoreFile.open() }
                }
                TextField {
                    id: restorePassphrase
                    Layout.fillWidth: true
                    echoMode: TextInput.Password
                    placeholderText: "Frase de la copia (mínimo 10 caracteres)"
                }
                Button {
                    Layout.fillWidth: true
                    text: "Restaurar identidad"
                    enabled: restoreFile.selectedFile.toString().length > 0 &&
                             restorePassphrase.text.length >= 10
                    onClicked: App.restoreIdentityBackup(
                        restoreFile.selectedFile, restorePassphrase.text)
                }
            }
        }

        Text {
            visible: App.lastError.length > 0
            Layout.fillWidth: true
            text: App.lastError
            color: "#FFD4DC"
            horizontalAlignment: Text.AlignHCenter
            wrapMode: Text.Wrap
        }
    }
}
