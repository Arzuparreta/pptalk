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
    property string replyBody: ""
    property string replyAuthor: ""
    readonly property bool systemEvent: author === "pptalk" && messageId.length === 0
    readonly property bool fileMessage: filePath.length > 0 || body.startsWith("📎 ")

    signal replyRequested(string messageId)
    signal editRequested(string messageId, string body)
    signal deleteRequested(string messageId)
    signal deleteLocalRequested(string messageId)
    signal openFileRequested(string path)

    width: ListView.view ? ListView.view.width : 600
    height: systemEvent ? 48 : bubble.height + 12

    Rectangle {
        visible: root.systemEvent
        anchors.centerIn: parent
        width: systemText.implicitWidth + 28
        height: 30
        radius: 15
        color: Theme.surface
        border.color: Theme.border
        Row {
            anchors.centerIn: parent
            spacing: 7
            AppIcon { name: "phone"; width: 14; height: 14; color: Theme.textSubtle }
            Text { id: systemText; text: root.body + " · " + root.time; color: Theme.textSubtle; font.pixelSize: 11 }
        }
    }

    Rectangle {
        id: bubble
        visible: !root.systemEvent
        width: Math.min(580, Math.max(190, bubbleContent.implicitWidth + 30))
        height: bubbleContent.implicitHeight + 24
        anchors.right: root.own ? parent.right : undefined
        anchors.left: root.own ? undefined : parent.left
        anchors.rightMargin: 28
        anchors.leftMargin: 28
        radius: Theme.radiusLarge
        color: root.own ? Theme.accentSoft : Theme.surfaceHigh
        border.color: root.highlighted ? Theme.warning
                    : (bubbleHover.containsMouse ? Theme.borderStrong
                       : (root.own ? "#3C487A" : Theme.border))
        border.width: root.highlighted ? 2 : 1

        Column {
            id: bubbleContent
            anchors.left: parent.left
            anchors.right: parent.right
            anchors.top: parent.top
            anchors.margins: 12
            spacing: 7

            Text {
                visible: !root.own
                text: root.author
                color: Theme.accent
                font.pixelSize: 11
                font.weight: Font.DemiBold
            }

            Rectangle {
                visible: root.replyTo.length > 0
                width: parent.width
                height: visible ? replyColumn.implicitHeight + 14 : 0
                radius: Theme.radiusSmall
                color: Qt.rgba(0.04, 0.06, 0.1, 0.38)
                border.color: Theme.accentStrong
                border.width: 0
                Rectangle { width: 3; radius: 2; height: parent.height; color: Theme.accent }
                Column {
                    id: replyColumn
                    x: 11
                    y: 7
                    width: parent.width - 20
                    spacing: 2
                    Text {
                        text: root.replyAuthor.length > 0 ? root.replyAuthor : "Respuesta"
                        color: Theme.accent
                        font.pixelSize: 10
                        font.weight: Font.DemiBold
                    }
                    Text {
                        width: parent.width
                        text: root.replyBody.length > 0 ? root.replyBody : "Mensaje anterior"
                        color: Theme.textMuted
                        font.pixelSize: 11
                        elide: Text.ElideRight
                    }
                }
            }

            Row {
                visible: root.fileMessage
                spacing: 9
                AppIcon { name: "file"; width: 20; height: 20; color: Theme.accent }
                Text {
                    width: Math.min(430, implicitWidth)
                    text: root.body.replace(/^📎\s*/, "")
                    color: Theme.text
                    font.pixelSize: 13
                    font.weight: Font.Medium
                    elide: Text.ElideMiddle
                }
            }

            Text {
                visible: !root.fileMessage
                width: Math.min(520, Math.max(160, implicitWidth))
                text: root.body
                color: root.deleted ? Theme.textSubtle : Theme.text
                font.italic: root.deleted
                wrapMode: Text.Wrap
                font.pixelSize: 14
                lineHeight: 1.15
            }

            Row {
                anchors.right: parent.right
                spacing: 6
                Text {
                    visible: root.edited
                    text: "editado"
                    color: Theme.textSubtle
                    font.pixelSize: 10
                }
                Text {
                    text: root.time
                    color: Theme.textSubtle
                    font.pixelSize: 10
                }
                Text {
                    visible: root.own
                    text: root.delivery === "delivered" ? "Entregado" :
                          (root.delivery === "direct" ? "Enviado · directo" :
                          (root.delivery === "mailbox" ? "Enviado · buzón" :
                          (root.delivery === "queued" ? "Pendiente" : "Enviado")))
                    color: root.delivery === "delivered" ? Theme.positive : Theme.textSubtle
                    font.pixelSize: 10
                }
            }
        }

        MouseArea {
            id: bubbleHover
            anchors.fill: parent
            acceptedButtons: Qt.LeftButton | Qt.RightButton
            hoverEnabled: true
            cursorShape: root.filePath.length > 0 ? Qt.PointingHandCursor : Qt.ArrowCursor
            onClicked: mouse => {
                if (mouse.button === Qt.RightButton) messageMenu.popup()
                else if (root.filePath.length > 0) root.openFileRequested(root.filePath)
            }
        }

        IconButton {
            visible: bubbleHover.containsMouse && !root.deleted && root.messageId.length > 0
            anchors.right: root.own ? undefined : parent.right
            anchors.left: root.own ? parent.left : undefined
            anchors.rightMargin: root.own ? 0 : -38
            anchors.leftMargin: root.own ? -38 : 0
            anchors.verticalCenter: parent.verticalCenter
            iconName: "reply"
            description: "Responder"
            buttonSize: 32
            onClicked: root.replyRequested(root.messageId)
        }

        AppMenu {
            id: messageMenu
            AppMenuItem {
                text: "Responder"
                enabled: !root.deleted && root.messageId.length > 0
                onTriggered: root.replyRequested(root.messageId)
            }
            AppMenuItem {
                text: "Editar"
                visible: root.own
                enabled: !root.deleted && root.messageId.length > 0
                onTriggered: root.editRequested(root.messageId, root.body)
            }
            AppMenuItem {
                text: "Eliminar para mí"
                visible: root.localDeleteAllowed
                enabled: !root.deleted && root.messageId.length > 0
                onTriggered: root.deleteLocalRequested(root.messageId)
            }
            AppMenuItem {
                text: "Eliminar para todos"
                visible: root.own
                enabled: !root.deleted && root.messageId.length > 0
                onTriggered: root.deleteRequested(root.messageId)
            }
        }
    }
}
