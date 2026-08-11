import QtQuick
import QtQuick.Controls

Button {
    id: root
    property string iconName: "more"
    property string description: ""
    property bool active: false
    property bool danger: false
    property int buttonSize: 40

    implicitWidth: buttonSize
    implicitHeight: buttonSize
    padding: 0
    hoverEnabled: true
    Accessible.name: description

    contentItem: AppIcon {
        name: root.iconName
        color: root.danger ? Theme.danger
             : (root.active ? Theme.text : (root.hovered ? Theme.text : Theme.textMuted))
        anchors.centerIn: parent
        width: 19
        height: 19
    }
    background: Rectangle {
        radius: Theme.radius
        color: root.down ? (root.danger ? Theme.dangerSoft : Theme.accentSoft)
             : root.active ? Theme.accentStrong
             : root.hovered ? Theme.surfaceHigh : "transparent"
        border.color: root.active ? Theme.accent : (root.hovered ? Theme.borderStrong : Theme.border)
        border.width: 1
        Behavior on color { ColorAnimation { duration: 110 } }
    }
    ToolTip.visible: hovered && description.length > 0
    ToolTip.text: description
    ToolTip.delay: 450
}
