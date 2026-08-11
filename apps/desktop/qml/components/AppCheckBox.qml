import QtQuick
import QtQuick.Controls

CheckBox {
    id: root
    spacing: 9
    implicitHeight: 38
    indicator: Rectangle {
        implicitWidth: 20
        implicitHeight: 20
        x: root.leftPadding
        y: (root.height - height) / 2
        radius: 6
        color: root.checked ? Theme.accentStrong : Theme.canvas
        border.color: root.checked ? Theme.accent : Theme.borderStrong
        AppIcon { anchors.centerIn: parent; visible: root.checked; name: "check"; color: "white"; width: 13; height: 13; strokeWidth: 2.2 }
    }
    contentItem: Text {
        text: root.text
        color: root.enabled ? Theme.text : Theme.textSubtle
        font.pixelSize: 12
        verticalAlignment: Text.AlignVCenter
        leftPadding: root.indicator.width + root.spacing
        elide: Text.ElideRight
    }
}
