import QtQuick
import QtQuick.Controls

Slider {
    id: root
    implicitHeight: 30
    background: Rectangle {
        x: root.leftPadding
        y: root.topPadding + root.availableHeight / 2 - height / 2
        implicitWidth: 180
        implicitHeight: 4
        width: root.availableWidth
        height: implicitHeight
        radius: 2
        color: Theme.border
        Rectangle {
            width: root.visualPosition * parent.width
            height: parent.height
            radius: 2
            color: Theme.accent
        }
    }
    handle: Rectangle {
        x: root.leftPadding + root.visualPosition * (root.availableWidth - width)
        y: root.topPadding + root.availableHeight / 2 - height / 2
        implicitWidth: 18
        implicitHeight: 18
        radius: 9
        color: root.pressed ? Theme.accent : "#FFFFFF"
        border.color: Theme.accentStrong
        border.width: 2
    }
}
