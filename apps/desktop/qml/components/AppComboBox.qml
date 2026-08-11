import QtQuick
import QtQuick.Controls

ComboBox {
    id: root
    implicitHeight: Theme.controlHeight
    leftPadding: 13
    rightPadding: 38
    font.pixelSize: 13

    contentItem: Text {
        text: root.displayText
        color: root.enabled ? Theme.text : Theme.textSubtle
        font: root.font
        verticalAlignment: Text.AlignVCenter
        elide: Text.ElideRight
    }
    indicator: AppIcon {
        name: "chevron-right"
        color: Theme.textMuted
        width: 16
        height: 16
        rotation: 90
        x: root.width - width - 13
        y: (root.height - height) / 2
    }
    background: Rectangle {
        radius: Theme.radius
        color: Theme.canvas
        border.color: root.activeFocus || root.popup.visible ? Theme.accent
                    : (root.hovered ? Theme.borderStrong : Theme.border)
        border.width: root.activeFocus || root.popup.visible ? 1.5 : 1
    }
    delegate: ItemDelegate {
        width: root.width
        implicitHeight: 38
        highlighted: root.highlightedIndex === index
        contentItem: Text {
            text: root.textRole.length > 0 ? model[root.textRole] : modelData
            color: highlighted ? Theme.text : Theme.textMuted
            font.pixelSize: 12
            verticalAlignment: Text.AlignVCenter
        }
        background: Rectangle {
            color: highlighted ? Theme.surfaceHigh : "transparent"
            radius: Theme.radiusSmall
        }
    }
    popup: Popup {
        y: root.height + 5
        width: root.width
        implicitHeight: contentItem.implicitHeight + 10
        padding: 5
        contentItem: ListView {
            clip: true
            implicitHeight: Math.min(contentHeight, 260)
            model: root.popup.visible ? root.delegateModel : null
            currentIndex: root.highlightedIndex
            ScrollIndicator.vertical: ScrollIndicator {}
        }
        background: Rectangle {
            color: Theme.elevated
            radius: Theme.radius
            border.color: Theme.borderStrong
        }
    }
}
