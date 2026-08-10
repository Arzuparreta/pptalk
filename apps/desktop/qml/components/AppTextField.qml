import QtQuick
import QtQuick.Controls

TextField {
    id: root
    implicitHeight: Theme.controlHeight
    color: Theme.text
    placeholderTextColor: Theme.textSubtle
    selectionColor: Theme.accentStrong
    selectedTextColor: "#FFFFFF"
    font.pixelSize: 13
    leftPadding: 13
    rightPadding: 13
    background: Rectangle {
        radius: Theme.radius
        color: Theme.canvas
        border.color: root.activeFocus ? Theme.accent : (root.hovered ? Theme.borderStrong : Theme.border)
        border.width: root.activeFocus ? 1.5 : 1
        Behavior on border.color { ColorAnimation { duration: 100 } }
    }
}
