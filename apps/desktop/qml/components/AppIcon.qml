import QtQuick

Item {
    id: root
    property string name: ""
    property color color: Theme.textMuted
    property real strokeWidth: 1.8
    implicitWidth: 20
    implicitHeight: 20

    Canvas {
        id: canvas
        anchors.fill: parent
        antialiasing: true
        onPaint: {
            const ctx = getContext("2d")
            const sx = width / 24
            const sy = height / 24
            ctx.reset()
            ctx.scale(sx, sy)
            ctx.strokeStyle = root.color
            ctx.fillStyle = root.color
            ctx.lineWidth = root.strokeWidth
            ctx.lineCap = "round"
            ctx.lineJoin = "round"

            function path(points, close) {
                ctx.beginPath()
                ctx.moveTo(points[0], points[1])
                for (let i = 2; i < points.length; i += 2) ctx.lineTo(points[i], points[i + 1])
                if (close) ctx.closePath()
                ctx.stroke()
            }
            function line(x1, y1, x2, y2) { path([x1, y1, x2, y2], false) }
            function circle(x, y, r) { ctx.beginPath(); ctx.arc(x, y, r, 0, Math.PI * 2); ctx.stroke() }
            function rect(x, y, w, h, r) {
                ctx.beginPath(); ctx.roundedRect(x, y, w, h, r, r); ctx.stroke()
            }

            switch (root.name) {
            case "search": circle(10.5, 10.5, 5.5); line(15, 15, 20, 20); break
            case "plus": line(12, 5, 12, 19); line(5, 12, 19, 12); break
            case "close": line(6, 6, 18, 18); line(18, 6, 6, 18); break
            case "more": circle(5, 12, 1); circle(12, 12, 1); circle(19, 12, 1); break
            case "send": path([4, 5, 21, 12, 4, 19, 7, 12, 4, 5], true); line(7, 12, 20, 12); break
            case "reply": path([10, 7, 4, 12, 10, 17], false); ctx.beginPath(); ctx.moveTo(5, 12); ctx.quadraticCurveTo(17, 10, 20, 18); ctx.stroke(); break
            case "attachment":
                ctx.beginPath(); ctx.moveTo(8, 12); ctx.lineTo(14.5, 5.5); ctx.arc(16.5, 7.5, 2.8, -2.35, 0.8); ctx.lineTo(8.5, 15.5); ctx.arc(6, 13, 3.5, 0.8, -2.35, true); ctx.lineTo(14, 5); ctx.stroke(); break
            case "phone":
            case "hangup":
                ctx.beginPath(); ctx.moveTo(7, 4); ctx.quadraticCurveTo(4, 5, 5, 9); ctx.quadraticCurveTo(7.5, 16.5, 15, 19); ctx.quadraticCurveTo(19, 20, 20, 17); ctx.lineTo(17, 14); ctx.quadraticCurveTo(16, 13, 14.5, 14.5); ctx.lineTo(9.5, 9.5); ctx.quadraticCurveTo(11, 8, 10, 7); ctx.closePath(); ctx.stroke();
                if (root.name === "hangup") line(5, 20, 20, 5)
                break
            case "mic":
            case "mic-off":
                rect(9, 3, 6, 12, 3); ctx.beginPath(); ctx.arc(12, 12, 7, 0, Math.PI); ctx.stroke(); line(12, 19, 12, 22); line(8, 22, 16, 22); if (root.name === "mic-off") line(4, 4, 20, 20); break
            case "camera":
                rect(3, 6, 13, 12, 2); path([16, 10, 21, 7, 21, 17, 16, 14], true); break
            case "screen": rect(3, 4, 18, 14, 2); line(8, 22, 16, 22); line(12, 18, 12, 22); line(12, 15, 12, 8); path([8, 12, 12, 8, 16, 12], false); break
            case "pause": rect(6, 5, 4, 14, 1); rect(14, 5, 4, 14, 1); break
            case "play": path([8, 5, 19, 12, 8, 19, 8, 5], true); break
            case "users": circle(9, 8, 3); circle(17, 9, 2.5); ctx.beginPath(); ctx.arc(9, 18, 6, Math.PI, Math.PI * 2); ctx.stroke(); ctx.beginPath(); ctx.arc(17, 18, 4.5, Math.PI, Math.PI * 2); ctx.stroke(); break
            case "user-add": circle(9, 8, 3); ctx.beginPath(); ctx.arc(9, 19, 6, Math.PI, Math.PI * 2); ctx.stroke(); line(18, 8, 18, 14); line(15, 11, 21, 11); break
            case "group": circle(8, 8, 3); circle(17, 8, 3); ctx.beginPath(); ctx.arc(8, 19, 5, Math.PI, Math.PI * 2); ctx.stroke(); ctx.beginPath(); ctx.arc(17, 19, 5, Math.PI, Math.PI * 2); ctx.stroke(); break
            case "settings": circle(12, 12, 3.2); circle(12, 12, 8); line(12, 2, 12, 5); line(12, 19, 12, 22); line(2, 12, 5, 12); line(19, 12, 22, 12); break
            case "check": path([4, 12, 9, 17, 20, 6], false); break
            case "copy": rect(8, 8, 11, 12, 2); rect(4, 4, 11, 12, 2); break
            case "pin": path([8, 3, 16, 3, 15, 9, 19, 13, 5, 13, 9, 9, 8, 3], true); line(12, 13, 12, 21); break
            case "archive": rect(4, 7, 16, 13, 2); rect(3, 4, 18, 4, 1); line(9, 12, 15, 12); break
            case "bell-off": ctx.beginPath(); ctx.arc(12, 11, 6, Math.PI, 0); ctx.lineTo(18, 17); ctx.lineTo(6, 17); ctx.closePath(); ctx.stroke(); line(10, 21, 14, 21); line(4, 4, 20, 20); break
            case "shield": path([12, 3, 20, 6, 19, 14, 12, 21, 5, 14, 4, 6, 12, 3], true); break
            case "trash": rect(6, 7, 12, 14, 2); line(4, 7, 20, 7); line(9, 3, 15, 3); line(10, 11, 10, 17); line(14, 11, 14, 17); break
            case "file": path([6, 3, 15, 3, 20, 8, 20, 21, 6, 21, 6, 3], true); path([15, 3, 15, 8, 20, 8], false); break
            case "lock": rect(5, 10, 14, 11, 2); ctx.beginPath(); ctx.arc(12, 10, 5, Math.PI, 0); ctx.stroke(); break
            case "device": rect(5, 3, 14, 18, 2); line(10, 18, 14, 18); break
            case "inbox": path([4, 5, 20, 5, 22, 15, 17, 15, 15, 19, 9, 19, 7, 15, 2, 15, 4, 5], true); break
            case "download": line(12, 3, 12, 15); path([7, 11, 12, 16, 17, 11], false); line(4, 21, 20, 21); break
            case "chevron-right": path([9, 5, 16, 12, 9, 19], false); break
            case "info": circle(12, 12, 9); line(12, 11, 12, 17); circle(12, 7, 0.5); break
            default: circle(12, 12, 8); break
            }
        }
        Connections {
            target: root
            function onNameChanged() { canvas.requestPaint() }
            function onColorChanged() { canvas.requestPaint() }
            function onWidthChanged() { canvas.requestPaint() }
            function onHeightChanged() { canvas.requestPaint() }
        }
        Component.onCompleted: requestPaint()
    }
}
