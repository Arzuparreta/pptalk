import QtQuick
import QtQuick.Controls

MenuItem {
    id: root
    implicitHeight: visible ? 38 : 0
    leftPadding: 12
    rightPadding: 12
    contentItem: Text {
        text: root.text
        color: root.enabled ? (root.highlighted ? Theme.text : Theme.textMuted) : Theme.textSubtle
        font.pixelSize: 12
        verticalAlignment: Text.AlignVCenter
    }
    background: Rectangle {
        radius: Theme.radiusSmall
        color: root.highlighted ? Theme.surfaceHigh : "transparent"
    }
}
