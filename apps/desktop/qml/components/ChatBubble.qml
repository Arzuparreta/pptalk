import QtQuick
import QtQuick.Controls

Item {
    id: root
    required property string author
    required property string body
    required property string time
    required property bool own
    required property string messageId
    required property string delivery
    required property bool edited
    required property bool deleted
    required property string replyTo
    required property string filePath
    required property bool localDeleteAllowed
    property bool highlighted: false
    signal replyRequested(string messageId)
    signal editRequested(string messageId, string body)
    signal deleteRequested(string messageId)
    signal deleteLocalRequested(string messageId)
    signal openFileRequested(string path)
    width: ListView.view ? ListView.view.width : 500
    height: bubble.height + 14

    Rectangle {
        id: bubble
        width: Math.min(520, Math.max(150, messageText.implicitWidth + 32))
        height: authorText.height + messageText.implicitHeight + metaText.height + 30
        anchors.right: root.own ? parent.right : undefined
        anchors.left: root.own ? undefined : parent.left
        anchors.rightMargin: 22
        anchors.leftMargin: 22
        radius: 16
        color: root.own ? "#5C50C9" : "#24212D"
        border.color: root.highlighted ? "#E8D978" : (root.own ? "#756AE0" : "#34303F")
        border.width: root.highlighted ? 2 : 1

        Text {
            id: authorText
            anchors.left: parent.left
            anchors.top: parent.top
            anchors.margins: 14
            text: root.author
            color: root.own ? "#DDD8FF" : "#AFA7FF"
            font.pixelSize: 12
            font.weight: Font.DemiBold
        }
        Text {
            id: messageText
            anchors.left: parent.left
            anchors.right: parent.right
            anchors.top: authorText.bottom
            anchors.leftMargin: 14
            anchors.rightMargin: 14
            anchors.topMargin: 5
            text: root.body + (root.edited ? "  (editado)" : "")
            color: root.deleted ? "#9B95A5" : "#F6F4FA"
            font.italic: root.deleted
            wrapMode: Text.Wrap
            font.pixelSize: 14
            lineHeight: 1.16
        }
        Text {
            id: metaText
            anchors.right: parent.right
            anchors.bottom: parent.bottom
            anchors.margins: 10
            text: root.time + (root.own ? "  " + (root.delivery === "delivered" ? "Entregado" :
                  (root.delivery === "direct" ? "Enviado · directo" :
                  (root.delivery === "queued" ? "Pendiente" : "Enviado"))) : "")
            color: root.own ? "#C5BFFF" : "#777283"
            font.pixelSize: 10
        }

        MouseArea {
            anchors.fill: parent
            acceptedButtons: Qt.LeftButton | Qt.RightButton
            cursorShape: root.filePath.length > 0 ? Qt.PointingHandCursor : Qt.ArrowCursor
            onClicked: mouse => {
                if (mouse.button === Qt.RightButton) messageMenu.popup()
                else if (root.filePath.length > 0) root.openFileRequested(root.filePath)
            }
        }

        Menu {
            id: messageMenu
            MenuItem {
                text: "Responder"
                enabled: !root.deleted && root.messageId.length > 0
                onTriggered: root.replyRequested(root.messageId)
            }
            MenuItem {
                text: "Editar"
                visible: root.own
                enabled: !root.deleted && root.messageId.length > 0
                onTriggered: root.editRequested(root.messageId, root.body)
            }
            MenuItem {
                text: "Eliminar para mí"
                visible: root.localDeleteAllowed
                enabled: !root.deleted && root.messageId.length > 0
                onTriggered: root.deleteLocalRequested(root.messageId)
            }
            MenuItem {
                text: "Eliminar para todos"
                visible: root.own
                enabled: !root.deleted && root.messageId.length > 0
                onTriggered: root.deleteRequested(root.messageId)
            }
        }
    }
}
