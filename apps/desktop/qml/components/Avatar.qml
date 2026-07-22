import QtQuick

Rectangle {
    id: root
    property string label: "?"
    property color accent: "#8B7CFF"
    property int size: 42
    width: size
    height: size
    radius: size / 2
    color: Qt.alpha(accent, 0.2)
    border.color: Qt.alpha(accent, 0.48)
    border.width: 1

    Text {
        anchors.centerIn: parent
        text: root.label.length > 0 ? root.label.charAt(0).toUpperCase() : "?"
        color: root.accent
        font.pixelSize: root.size * 0.38
        font.weight: Font.DemiBold
    }
}
