import QtQuick

Rectangle {
    id: root
    property string label: "?"
    property color accent: "#8B7CFF"
    property int size: 42
    property string source: ""
    width: size
    height: size
    radius: size / 2
    color: Qt.alpha(accent, 0.2)
    border.color: Qt.alpha(accent, 0.48)
    border.width: 1
    clip: true

    Image {
        anchors.fill: parent
        visible: root.source.length > 0
        source: root.source
        fillMode: Image.PreserveAspectCrop
        asynchronous: true
    }

    Text {
        anchors.centerIn: parent
        visible: root.source.length === 0
        text: root.label.length > 0 ? root.label.charAt(0).toUpperCase() : "?"
        color: root.accent
        font.pixelSize: root.size * 0.38
        font.weight: Font.DemiBold
    }
}
