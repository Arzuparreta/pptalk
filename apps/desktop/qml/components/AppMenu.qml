import QtQuick
import QtQuick.Controls

Menu {
    id: root
    padding: 6
    overlap: 4
    background: Rectangle {
        implicitWidth: 230
        color: Theme.elevated
        radius: Theme.radius
        border.color: Theme.borderStrong
        border.width: 1
    }
}
