import QtQuick
import QtQuick.Controls

Switch {
    text: "Abrir pptalk al iniciar Windows"
    checked: App.autostartEnabled
    onToggled: App.autostartEnabled = checked
}
