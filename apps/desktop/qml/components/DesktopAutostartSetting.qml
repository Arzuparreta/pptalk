import QtQuick
import QtQuick.Controls

Switch {
    text: "Abrir pptalk al iniciar sesión"
    checked: App.autostartEnabled
    onToggled: App.autostartEnabled = checked
}
