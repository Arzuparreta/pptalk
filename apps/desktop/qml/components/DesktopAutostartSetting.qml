import QtQuick

AppSwitch {
    text: "Abrir pptalk al iniciar sesión"
    checked: App.autostartEnabled
    onToggled: App.autostartEnabled = checked
}
