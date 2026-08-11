import QtQuick
import QtQuick.Controls

Switch {
    id: root
    spacing: 10
    implicitHeight: Theme.controlHeight
    indicator: Rectangle {
        implicitWidth: 42
        implicitHeight: 24
        x: root.leftPadding
        y: (root.height - height) / 2
        radius: height / 2
        color: root.checked ? Theme.accentStrong : Theme.elevated
        border.color: root.checked ? Theme.accent : Theme.borderStrong
        Rectangle {
            width: 18
            height: 18
            radius: 9
            y: 3
            x: root.checked ? parent.width - width - 3 : 3
            color: root.checked ? "#FFFFFF" : Theme.textMuted
            Behavior on x { NumberAnimation { duration: 130; easing.type: Easing.OutCubic } }
        }
    }
    contentItem: Text {
        text: root.text
        color: root.enabled ? Theme.text : Theme.textSubtle
        font.pixelSize: 13
        verticalAlignment: Text.AlignVCenter
        leftPadding: root.indicator.width + root.spacing
    }
}
