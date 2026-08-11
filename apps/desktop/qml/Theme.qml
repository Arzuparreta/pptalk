pragma Singleton
import QtQuick

QtObject {
    readonly property color canvas: "#090C12"
    readonly property color sidebar: "#0D1119"
    readonly property color surface: "#121722"
    readonly property color surfaceHigh: "#181F2C"
    readonly property color elevated: "#1D2533"
    readonly property color border: "#273244"
    readonly property color borderStrong: "#3A475D"

    readonly property color text: "#F4F7FC"
    readonly property color textMuted: "#9AA6B8"
    readonly property color textSubtle: "#69768A"

    readonly property color accent: "#8B9BFF"
    readonly property color accentStrong: "#6F80F6"
    readonly property color accentSoft: "#252E52"
    readonly property color positive: "#50D5A7"
    readonly property color positiveSoft: "#17342F"
    readonly property color warning: "#F2BC66"
    readonly property color danger: "#FF788A"
    readonly property color dangerSoft: "#3B2029"

    readonly property int radiusSmall: 8
    readonly property int radius: 12
    readonly property int radiusLarge: 18
    readonly property int controlHeight: 40
    readonly property int sidebarWidth: 304
}
