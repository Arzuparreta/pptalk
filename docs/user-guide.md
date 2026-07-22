# Guía de uso

## La primera vez

Al abrir pptalk se crea una identidad en el dispositivo. No tienes que elegir
usuario global, correo ni contraseña. El nombre inicial se toma del usuario del
sistema y solo sirve para que tus contactos te reconozcan.

La ventana tiene dos zonas:

- a la izquierda están tus contactos y grupos;
- a la derecha está la conversación seleccionada, sus mensajes y sus llamadas.

Si todavía no tienes contactos, empieza con el botón **+** de la esquina
superior izquierda.

## Conectar con otra persona

### Si tú invitas

1. Abre el botón **+** superior.
2. pptalk crea un enlace que empieza por `pptalk://contact/`.
3. Pulsa **Copiar enlace**.
4. Envíalo a la persona por un medio de confianza.
5. Mantén pptalk abierto hasta que la otra persona lo acepte.

### Si recibes la invitación

1. Copia el enlace completo que te han enviado.
2. Abre pptalk y pulsa **+**.
3. Pega el enlace en “O pega una invitación que te hayan enviado”.
4. Pulsa **Aceptar**.

El contacto aparecerá en la lista. La invitación caduca y es de un solo uso. Si
ha expirado, el remitente debe generar otra. Envíala por un canal de confianza:
la interfaz todavía no ofrece una pantalla para comparar huellas, así que esta
versión no es adecuada si necesitas verificar una identidad de alto riesgo.

## Mensajes y archivos

Selecciona un contacto o grupo en la columna izquierda.

- `Enter` envía el mensaje.
- `Shift + Enter` inserta una nueva línea.
- El botón **➤** también envía.
- El botón **+** situado junto al mensaje abre el selector de archivos.

Los archivos se cifran antes de salir del dispositivo. Si el receptor no está
disponible, pptalk intenta entregarlos cuando vuelva a existir una ruta.

La etiqueta de la parte superior indica la ruta actual. “P2P directo” significa
que existe conexión directa; “E2EE · ruta automática” significa que el contenido
sigue cifrado de extremo a extremo aunque la ruta pueda pasar por infraestructura
intermedia.

## Crear y administrar un grupo

1. Añade primero a cada participante como contacto.
2. Pulsa **◫** en la parte superior izquierda.
3. Escribe un nombre para el grupo.
4. Escribe los nombres exactos de los contactos, separados por comas.
5. Pulsa **Crear con MLS**.

MLS es el sistema que cambia las claves cifradas del grupo cuando entra o sale
alguien; no tienes que configurarlo manualmente.

El creador administra la membresía. Dentro del grupo, el botón **⋯** permite
añadir o expulsar un contacto escribiendo su nombre exacto.

Una persona añadida más tarde no recibe el historial anterior a su entrada. Al
expulsar a una persona o revocar uno de sus dispositivos, el grupo cambia sus
claves para los mensajes siguientes.

## Llamadas

Pulsa el icono de **teléfono** de la conversación seleccionada:

- **Abrir sala sin llamar** crea una sala a la que los demás pueden entrar sin
  hacer sonar sus dispositivos.
- **Llamar** o **Llamar al grupo** envía una llamada visible.

Durante la llamada puedes activar o desactivar:

- micrófono: botón **◉**;
- cámara: botón **▣**;
- compartir pantalla: botón **↗**.

La cámara y la pantalla comienzan apagadas. **Salir** abandona la llamada.

En **⚙ → Calidad de cámara y pantalla** puedes dejar calidad automática o fijar
una resolución. Si eliges un modo manual que el equipo no puede cumplir, pptalk
muestra el error en vez de reducir la calidad silenciosamente.

## Mensajes cuando alguien está desconectado

Sin buzón, pptalk conserva localmente los envíos pendientes y vuelve a
intentarlos cuando los dos dispositivos pueden encontrarse.

Un buzón opcional permite depositar temporalmente mensajes ya cifrados mientras
el receptor está desconectado. Para configurarlo:

1. abre **⚙**;
2. pega la URL en **Nodo de buzón cifrado**;
3. pulsa **Guardar**.

Déjalo vacío si quieres usar solo rutas P2P. El nodo local que arranca
`scripts/dev.sh` sirve para desarrollo en la misma máquina; para amigos en
Internet necesitas una URL accesible por ambos. Consulta
[montar un buzón propio](self-hosting.md).

## Usar la identidad en otro dispositivo

Cada dispositivo tiene su propia clave. Vincular uno nuevo lo autoriza como
parte de tu identidad sin copiar sus claves internas.

En el dispositivo ya autorizado:

1. abre **⚙ → Vincular otro dispositivo**;
2. escribe un nombre, por ejemplo “Portátil”;
3. pulsa **Generar enlace (10 min)** y después **Copiar**;
4. transfiere el enlace al dispositivo nuevo por un canal de confianza.

En el dispositivo nuevo, antes de que tenga otro perfil, inicia pptalk así:

```sh
PPTALK_DEVICE_LINK='pptalk://device/v1#…' ./scripts/dev.sh start
```

El enlace caduca a los diez minutos. En **Dispositivos autorizados** puedes
revocar cualquier dispositivo salvo el que estás usando. La revocación no borra
remotamente un ordenador perdido; impide que siga participando con una identidad
válida y actualiza los grupos que controlas.

## Qué debes guardar

No existe recuperación central. Si pierdes todos los dispositivos y todas las
copias de sus datos, pierdes la identidad y el historial que no hayas exportado.

Protege el directorio de datos con cifrado de disco y una sesión de sistema
bloqueada. No envíes `profile.json`, bases de datos ni enlaces de dispositivo a
personas que no deban controlar tu identidad. Para hacer una copia consistente,
cierra primero pptalk con `./scripts/dev.sh stop` y copia entonces el directorio
completo de datos.

## Problemas comunes

### La aplicación no abre

```sh
./scripts/dev.sh doctor
./scripts/dev.sh logs desktop
./scripts/dev.sh restart
```

Comprueba que Qt y GStreamer cumplen las versiones de la
[guía de instalación](installation.md).

### El contacto no aparece

- Confirma que ambos tenéis pptalk abierto durante la aceptación inicial.
- Comprueba que el enlace está completo y no ha caducado.
- Genera una invitación nueva; las anteriores solo funcionan una vez.

### Un mensaje no llega

- Mira la etiqueta de conexión de la conversación.
- Mantén ambos clientes abiertos para forzar un nuevo intento directo.
- Si esperas entrega sin conexión, comprueba que el buzón configurado es
  accesible desde Internet y no solo desde `127.0.0.1`.

### Aparece un error de base de datos o MLS

No borres el perfil. Reinicia con la versión más reciente y guarda el mensaje
completo de `./scripts/dev.sh logs desktop`. Las migraciones de formatos locales
se aplican al abrir; conservar el perfil permite diagnosticar y recuperar el
estado.

### El script dice que un puerto ya está ocupado

Ejecuta `./scripts/dev.sh status`. El script se niega a cerrar procesos que no
pertenezcan a este repositorio. Si otro programa usa `9464`, cambia juntos la
dirección y la URL del nodo siguiendo `./scripts/dev.sh help`.
