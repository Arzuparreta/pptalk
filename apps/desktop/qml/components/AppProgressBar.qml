import QtQuick
import QtQuick.Controls

ProgressBar {
    id: root
    implicitHeight: 5
    background: Rectangle { radius: 3; color: Theme.border }
    contentItem: Item {
        implicitWidth: 160
        Rectangle {
            width: root.visualPosition * parent.width
            height: parent.height
            radius: 3
            color: Theme.accent
        }
    }
}
