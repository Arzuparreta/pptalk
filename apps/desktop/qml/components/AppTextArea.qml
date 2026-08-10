import QtQuick
import QtQuick.Controls

TextArea {
    id: root
    color: Theme.text
    placeholderTextColor: Theme.textSubtle
    selectionColor: Theme.accentStrong
    selectedTextColor: "#FFFFFF"
    font.pixelSize: 13
    leftPadding: 13
    rightPadding: 13
    topPadding: 11
    bottomPadding: 11
    wrapMode: TextEdit.Wrap
    background: Rectangle {
        radius: Theme.radius
        color: Theme.canvas
        border.color: root.activeFocus ? Theme.accent : (root.hovered ? Theme.borderStrong : Theme.border)
        border.width: root.activeFocus ? 1.5 : 1
    }
}
