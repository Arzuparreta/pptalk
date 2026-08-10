import QtQuick
import QtQuick.Controls

Dialog {
    id: root
    modal: true
    dim: true
    closePolicy: Popup.CloseOnEscape
    padding: 22
    anchors.centerIn: parent

    Overlay.modal: Rectangle { color: "#9906090E" }
    background: Rectangle {
        color: Theme.surfaceHigh
        radius: 20
        border.color: Theme.borderStrong
        border.width: 1
    }
    header: Rectangle {
        implicitHeight: root.title.length > 0 ? 62 : 0
        visible: implicitHeight > 0
        color: "transparent"
        Text {
            anchors.left: parent.left
            anchors.leftMargin: 22
            anchors.verticalCenter: parent.verticalCenter
            text: root.title
            color: Theme.text
            font.pixelSize: 18
            font.weight: Font.DemiBold
        }
        IconButton {
            anchors.right: parent.right
            anchors.rightMargin: 14
            anchors.verticalCenter: parent.verticalCenter
            iconName: "close"
            description: "Cerrar"
            buttonSize: 34
            onClicked: root.close()
        }
    }
}
