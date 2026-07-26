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
4. Pulsa **Revisar invitación**.
5. Comprueba el nombre y la caducidad y confirma **Aceptar**.

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

Los contactos y los mensajes se recuerdan al cerrar la aplicación. Pulsa con el
botón derecho sobre un mensaje para **Responder**, **Editar** o **Eliminar para
todos**. La búsqueda de la columna izquierda busca texto en todo el historial
local. “Entregado” confirma que el otro dispositivo guardó el mensaje; pptalk no
envía confirmaciones de lectura.

El menú **⋯** de un contacto permite fijar, archivar o silenciar la conversación,
ocultar tu presencia para esa persona, bloquearla o eliminarla. El historial
local no se borra al eliminar un contacto. Para recuperarlo como contacto hace
falta aceptar una invitación nueva.

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

Un grupo admite hasta 16 miembros. El propietario puede nombrar administradores,
transferir la propiedad o disolver el grupo. Propietario y administradores pueden
añadir o expulsar miembros normales desde **⋯**. Una llamada admite hasta ocho
participantes.

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

El micrófono, la cámara y la pantalla permanecen apagados mientras suena la
llamada; el micrófono se activa al aceptar. Si nadie responde, la llamada termina
a los 30 segundos y queda como perdida. **Ⅱ** retiene una llamada y **Salir** la
abandona. Las llamadas contestadas, rechazadas y perdidas quedan en el historial
local de la conversación.

En **⚙ → Calidad de cámara y pantalla** puedes dejar calidad automática o fijar
una resolución. Si eliges un modo manual que el equipo no puede cumplir, pptalk
muestra el error en vez de reducir la calidad silenciosamente.

## Mensajes cuando alguien está desconectado

pptalk conserva localmente los envíos pendientes y vuelve a intentarlos cuando
los dos dispositivos pueden encontrarse. La aplicación no pide una URL de buzón
ni depende de un servidor del proyecto. La ruta distribuida para entregar con
ambos clientes desconectados aún no ha superado la prueba de viabilidad, por lo
que esta versión no promete entrega offline.

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

Al coincidir en línea, los dispositivos autorizados sincronizan el texto de los
chats directos. Los archivos antiguos no se copian; solo se conserva su mensaje.
Una identidad admite como máximo cinco dispositivos activos.

## Ajustes

**No molestar** es manual y no caduca solo. **Micrófono al entrar** permite elegir
**Micrófono abierto** o **Pulsar para hablar**. En Windows
aparece **Abrir pptalk al iniciar Windows**, desactivado de forma predeterminada;
al activarlo abre la ventana principal. Ese ajuste no existe en Linux.
En **Pulsar para hablar**, mantén `Ctrl + Espacio` durante la llamada. Cuando hay
una versión publicada más nueva, **Ajustes** muestra su descarga; los paquetes
AUR se actualizan con el gestor de paquetes.

## Pérdida del dispositivo

No existe recuperación central ni una copia de seguridad soportada por la app.
Si pierdes todos los dispositivos, pierdes la identidad. Protege la sesión del
sistema y su disco; no envíes `profile.json`, bases de datos ni enlaces de
dispositivo a nadie que no deba controlar tu identidad.

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
- Si la otra persona está desconectada, espera a que ambos clientes vuelvan a
  estar abiertos: esta versión no promete entrega offline distribuida.

### Aparece un error de base de datos o MLS

No borres el perfil. Reinicia con la versión más reciente y guarda el mensaje
completo de `./scripts/dev.sh logs desktop`. Las migraciones de formatos locales
se aplican al abrir; conservar el perfil permite diagnosticar y recuperar el
estado.
