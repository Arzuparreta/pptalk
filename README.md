# pptalk

Habla con tus amigos sin una cuenta central y sin entregar tus conversaciones a
una empresa. pptalk es una aplicación nativa de escritorio para mensajes,
archivos, grupos y llamadas cifradas entre los dispositivos de sus usuarios.

> **Estado actual:** funciona como versión de desarrollo, pero todavía no es un
> producto auditado ni tiene una entrega estable publicada. No lo uses aún si una
> vulnerabilidad pudiera poner a alguien en peligro.

## Empezar

Necesitas Linux, Rust 1.91+, CMake, Ninja, Qt 6.8+ y GStreamer 1.24+. La
[guía de instalación](docs/installation.md) explica los requisitos y también el
proceso para Windows.

```sh
git clone https://github.com/Arzuparreta/pptalk.git
cd pptalk
./scripts/dev.sh start
```

La primera compilación puede tardar varios minutos. Al terminar se abrirá la
aplicación: escribe cómo quieres que te vean tus amigos para crear una identidad
local, o elige **Vincular equipo** o **Restaurar copia**. No hay registro ni
correo electrónico.

Para cerrar todo:

```sh
./scripts/dev.sh stop
```

## Añadir a un amigo

Los dos debéis tener pptalk abierto durante la primera conexión.

1. Pulsa **+** en la esquina superior izquierda.
2. Enseña el QR o pulsa **Copiar enlace** y envíaselo a tu amigo por un canal de
   confianza.
3. Tu amigo abre su propio botón **+**, pega el enlace, pulsa **Revisar invitación**
   y confirma tu nombre.
4. La conversación aparecerá en la columna izquierda.

El enlace caduca y solo se puede aceptar una vez. No lo publiques: durante su
vigencia funciona como una invitación privada.

## Hablar

- Escribe abajo y pulsa `Enter` para enviar. `Shift + Enter` añade una línea.
- Pulsa el **+** junto al cuadro de texto para enviar un archivo cifrado.
- También puedes arrastrar archivos sobre la conversación.
- Pulsa el **teléfono** para llamar o abrir una sala sin hacer sonar al otro.
- Pulsa **◫** arriba a la izquierda para crear un grupo con contactos existentes.
- Pulsa **⚙** para cambiar tu perfil, configurar llamadas o vincular otro dispositivo.

La [guía de uso](docs/user-guide.md) explica cada pantalla, los grupos, las
llamadas, el uso sin conexión y la vinculación de dispositivos.

## Qué significa “peer to peer” aquí

Cuando es posible, los dispositivos se conectan directamente. Si la red lo
impide, pueden usar transporte intermedio cifrado. Ese transporte puede ver que
existe tráfico, pero no el contenido.

No hay que configurar un servidor ni enviar un código en cada llamada. Los
contactos aceptados y el historial se conservan localmente. Si el destinatario
está desconectado, el envío queda pendiente en tu dispositivo hasta que vuelve a
existir una ruta; el almacenamiento distribuido sin conexión todavía está en
evaluación y no se anuncia como disponible.

Tus claves y tu historial viven en tus dispositivos. En **Ajustes** puedes
proteger la clave del historial con el almacén seguro del sistema y crear una
copia cifrada de tu identidad. No existe un botón corporativo de recuperación:
conserva la frase y el archivo de copia por separado.

## Si algo falla

```sh
./scripts/dev.sh status
./scripts/dev.sh logs -f
./scripts/dev.sh doctor
./scripts/dev.sh restart
```

Consulta [problemas comunes](docs/user-guide.md#problemas-comunes) antes de
borrar ningún perfil. La actualización de datos locales es automática; un error
de formato no significa que debas perder tu identidad.

## Documentación

Para usar pptalk:

- [Instalación](docs/installation.md)
- [Guía de uso](docs/user-guide.md)
- [Seguridad y límites](docs/threat-model.md)

Para desarrollar pptalk:

- [Desarrollo y pruebas](docs/development.md)
- [Arquitectura](docs/architecture.md)
- [Protocolo](docs/protocol.md)
- [Política de seguridad](SECURITY.md)

## Licencia

El cliente y las bibliotecas compartidas usan `GPL-3.0-or-later`. El nodo de
desarrollo legado usa `AGPL-3.0-or-later`; el spike Veilid aislado usa
`MPL-2.0`. Consulta [LICENSE](LICENSE) y [LICENSES](LICENSES/).
