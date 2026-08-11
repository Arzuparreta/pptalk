import QtQuick

Rectangle {
    id: root
    property string label: "?"
    property color accent: Theme.accent
    property int size: 42
    property string source: ""
    property bool online: false
    width: size
    height: size
    radius: Math.round(size * 0.34)
    color: Qt.tint(Theme.surfaceHigh, Qt.rgba(accent.r, accent.g, accent.b, 0.16))
    border.color: Qt.rgba(accent.r, accent.g, accent.b, 0.48)
    border.width: 1

    Image {
        anchors.fill: parent
        anchors.margins: 1
        visible: root.source.length > 0
        source: root.source
        fillMode: Image.PreserveAspectCrop
        asynchronous: true
        sourceSize.width: root.size * 2
        sourceSize.height: root.size * 2
        clip: true
    }

    Text {
        anchors.centerIn: parent
        visible: root.source.length === 0
        text: root.label.length > 0 ? root.label.charAt(0).toUpperCase() : "?"
        color: root.accent
        font.pixelSize: Math.round(root.size * 0.38)
        font.weight: Font.DemiBold
    }

    Rectangle {
        visible: root.online
        width: Math.max(10, root.size * 0.25)
        height: width
        radius: width / 2
        anchors.right: parent.right
        anchors.bottom: parent.bottom
        anchors.rightMargin: -2
        anchors.bottomMargin: -2
        color: Theme.positive
        border.color: Theme.sidebar
        border.width: 2
    }
}
