import QtQuick
import QtQuick.Controls

Button {
    id: root
    property string glyph: ""
    property bool active: false
    implicitWidth: 40
    implicitHeight: 40
    hoverEnabled: true

    contentItem: Text {
        text: root.glyph
        color: root.active ? "#F4F2FF" : "#B7B3C9"
        horizontalAlignment: Text.AlignHCenter
        verticalAlignment: Text.AlignVCenter
        font.pixelSize: 17
    }
    background: Rectangle {
        radius: 12
        color: root.active ? "#6658D9" : (root.hovered ? "#292635" : "transparent")
        border.color: root.active ? "#877BEE" : "#373342"
        border.width: root.active || root.hovered ? 1 : 0
    }
}
