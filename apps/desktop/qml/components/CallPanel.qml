import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

SectionCard {
    id: root
    signal showParticipants()
    readonly property bool compactLayout: width < 760
    implicitHeight: 84
    color: Theme.surface
    border.color: App.callState === "held" ? "#6B5735" : Theme.border

    RowLayout {
        anchors.fill: parent
        anchors.leftMargin: 16
        anchors.rightMargin: 14
        spacing: 12

        Rectangle {
            width: 44; height: 44; radius: 14
            color: App.callState === "held" ? "#332B20" : Theme.positiveSoft
            AppIcon {
                anchors.centerIn: parent
                name: App.callState === "held" ? "pause" : "phone"
                color: App.callState === "held" ? Theme.warning : Theme.positive
                width: 20; height: 20
            }
        }
        ColumnLayout {
            Layout.preferredWidth: root.compactLayout ? 130 : 170
            spacing: 2
            Text {
                text: App.callState === "calling" ? "Llamando a " + App.callContact
                    : App.callState === "held" ? "En espera · " + App.callContact
                    : "En llamada · " + App.callContact
                color: Theme.text
                font.pixelSize: 13
                font.weight: Font.DemiBold
                elide: Text.ElideRight
                Layout.fillWidth: true
            }
            Text {
                text: App.callState === "held" ? "El audio está pausado" : "Conexión cifrada de extremo a extremo"
                color: Theme.textMuted
                font.pixelSize: 10
            }
        }
        Item { Layout.fillWidth: true }
        ActionButton {
            visible: !root.compactLayout && App.callState !== "held" && App.callParticipants.length > 0
            compact: true
            text: App.callParticipants.length + (App.callParticipants.length === 1 ? " persona" : " personas")
            iconName: "users"
            onClicked: root.showParticipants()
        }
        IconButton {
            visible: root.compactLayout && App.callState !== "held" && App.callParticipants.length > 0
            iconName: "users"
            description: App.callParticipants.length + (App.callParticipants.length === 1 ? " persona" : " personas")
            onClicked: root.showParticipants()
        }
        ActionButton {
            visible: !root.compactLayout && App.callState !== "held"
            compact: true
            text: App.microphoneEnabled ? "Micrófono" : "Silenciado"
            iconName: App.microphoneEnabled ? "mic" : "mic-off"
            kind: App.microphoneEnabled ? "secondary" : "danger"
            onClicked: App.toggleMicrophone()
        }
        IconButton {
            visible: root.compactLayout && App.callState !== "held"
            iconName: App.microphoneEnabled ? "mic" : "mic-off"
            description: App.microphoneEnabled ? "Silenciar micrófono" : "Activar micrófono"
            active: App.microphoneEnabled
            danger: !App.microphoneEnabled
            onClicked: App.toggleMicrophone()
        }
        ActionButton {
            visible: !root.compactLayout && App.callState === "connected"
            compact: true
            text: "Pausa"
            iconName: "pause"
            description: "Poner llamada en espera"
            onClicked: App.holdCall()
        }
        IconButton {
            visible: root.compactLayout && App.callState === "connected"
            iconName: "pause"
            description: "Poner llamada en espera"
            onClicked: App.holdCall()
        }
        ActionButton {
            visible: App.callState === "held"
            compact: true
            text: "Reanudar"
            iconName: "play"
            kind: "primary"
            onClicked: App.resumeHeldCall()
        }
        IconButton {
            visible: App.callState !== "held"
            iconName: "camera"
            description: App.cameraEnabled ? "Apagar cámara" : "Encender cámara"
            active: App.cameraEnabled
            onClicked: App.toggleCamera()
        }
        IconButton {
            visible: App.callState !== "held"
            iconName: "screen"
            description: App.sharingScreen ? "Dejar de compartir" : "Compartir pantalla"
            active: App.sharingScreen
            onClicked: App.toggleScreenShare()
        }
        ActionButton {
            compact: true
            text: "Salir"
            iconName: "hangup"
            kind: "danger"
            onClicked: App.leaveCall()
        }
    }
}
