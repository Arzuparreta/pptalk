import QtQuick

Item {
    id: root
    required property string author
    required property string body
    required property string time
    required property bool own
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
        border.color: root.own ? "#756AE0" : "#34303F"

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
            text: root.body
            color: "#F6F4FA"
            wrapMode: Text.Wrap
            font.pixelSize: 14
            lineHeight: 1.16
        }
        Text {
            id: metaText
            anchors.right: parent.right
            anchors.bottom: parent.bottom
            anchors.margins: 10
            text: root.time + (root.own ? "  ✓✓" : "")
            color: root.own ? "#C5BFFF" : "#777283"
            font.pixelSize: 10
        }
    }
}
