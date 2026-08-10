import QtQuick
import QtQuick.Controls

Button {
    id: root
    property string iconName: ""
    property string kind: "secondary"
    property string description: ""
    property bool compact: false

    implicitHeight: compact ? 34 : Theme.controlHeight
    implicitWidth: contentRow.implicitWidth + (compact ? 24 : 30)
    leftPadding: compact ? 12 : 15
    rightPadding: leftPadding
    hoverEnabled: true
    Accessible.name: description.length > 0 ? description : text

    contentItem: Row {
        id: contentRow
        spacing: 8
        anchors.centerIn: parent
        AppIcon {
            visible: root.iconName.length > 0
            name: root.iconName
            width: root.compact ? 16 : 18
            height: width
            color: root.enabled ? (root.kind === "danger" ? Theme.danger :
                   (root.kind === "primary" ? "#FFFFFF" : Theme.textMuted)) : Theme.textSubtle
            anchors.verticalCenter: parent.verticalCenter
        }
        Text {
            text: root.text
            color: root.enabled ? (root.kind === "danger" ? Theme.danger :
                   (root.kind === "primary" ? "#FFFFFF" : Theme.text)) : Theme.textSubtle
            font.pixelSize: root.compact ? 12 : 13
            font.weight: Font.DemiBold
            anchors.verticalCenter: parent.verticalCenter
        }
    }
    background: Rectangle {
        radius: Theme.radius
        color: {
            if (!root.enabled) return Theme.surface
            if (root.kind === "primary") return root.down ? "#5869DE" : (root.hovered ? "#7989FA" : Theme.accentStrong)
            if (root.kind === "danger") return root.down ? "#47212B" : (root.hovered ? Theme.dangerSoft : "transparent")
            if (root.kind === "ghost") return root.hovered ? Theme.surfaceHigh : "transparent"
            return root.down ? Theme.elevated : (root.hovered ? Theme.surfaceHigh : Theme.surface)
        }
        border.color: !root.enabled ? Theme.border
                    : root.kind === "primary" ? Theme.accentStrong
                    : root.kind === "danger" ? (root.hovered ? Theme.danger : Theme.border)
                    : root.hovered ? Theme.borderStrong : Theme.border
        border.width: root.kind === "ghost" && !root.hovered ? 0 : 1
        Behavior on color { ColorAnimation { duration: 110 } }
    }
    ToolTip.visible: hovered && description.length > 0
    ToolTip.text: description
    ToolTip.delay: 500
}
