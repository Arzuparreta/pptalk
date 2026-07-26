# Arquitectura

Esta página explica cómo está construido pptalk. Para usar la aplicación no
necesitas conocer estos detalles; empieza por la [guía de uso](user-guide.md).

## Idea principal

pptalk no tiene una base de datos central de usuarios. Cada dispositivo genera
sus claves y mantiene una copia local de las conversaciones que le corresponden.
Los contactos se descubren mediante invitaciones privadas, no buscando nombres
en un directorio mundial.

En términos de producto:

- una identidad es un registro criptográfico compartido por sus dispositivos;
- un contacto es una identidad aceptada mediante una invitación;
- una conversación es un registro cifrado independiente;
- el propietario y sus administradores controlan quién entra y sale;
- un nodo opcional transporta datos opacos, pero no decide identidades ni
  membresías.

No existe un `tenant_id` asignado por un servidor. El límite equivalente a un
tenant nace de las claves de identidad y de la membresía de cada conversación.

## Recorrido de un mensaje

```text
Interfaz Qt
    ↓
daemon local
    ↓
firma y cifrado ─────→ historial SQLCipher
    ↓                         ↓
transporte QUIC        outbox para reintentos
    ↓
conexión directa o ruta cifrada opcional
    ↓
dispositivo del contacto
```

El receptor valida autor, dispositivo, secuencia y membresía antes de guardar
el evento. Los identificadores hacen que recibir dos veces el mismo mensaje sea
inofensivo.

## Identidad y dispositivos

El primer dispositivo crea:

1. una clave de firma Ed25519;
2. una identidad con un evento inicial;
3. una identidad de transporte Iroh;
4. una base local SQLCipher.

Un segundo dispositivo genera sus propias claves y recibe una autorización
firmada por uno ya válido. No se clona una identidad MLS. Los dispositivos de la
misma identidad sincronizan el texto de chats directos, pero no adjuntos
antiguos. Al revocarlo, deja de recibir envíos y los grupos controlados avanzan a
claves que ya no lo incluyen.

El perfil conserva compatibilidad con instalaciones anteriores. Cuando el
usuario activa **Protección local**, la clave SQLCipher se escribe y se lee de
vuelta desde el almacén seguro del sistema antes de sustituirla por ceros en
`profile.json`. La copia de identidad usa Argon2id y XChaCha20-Poly1305; se
restaura únicamente sobre una ruta de perfil nueva.

Las invitaciones de contacto contienen una capacidad de un solo uso, una prueba
firmada de dirección y una caducidad. La interfaz deriva una huella estable de la
identidad y conserva por separado la comprobación manual del usuario; aceptar
una prueba criptográfica válida no marca automáticamente a la persona como
verificada.

## Conversaciones y sincronización

Cada autor mantiene una secuencia monotónica por dispositivo. Al reconectar, los
peers comparan sus fronteras causales y transmiten únicamente los eventos que
faltan.

Si ninguna ruta está disponible, el sobre cifrado permanece en el outbox del
emisor. El cliente de escritorio no configura buzones. El prototipo Veilid está
aislado en `crates/distributed`: publica datos reales, pero la recuperación desde
un segundo nodo provoca un fallo de apagado reproducible en Veilid 0.5.7. Por eso
no forma parte de la ruta del producto.

Un miembro nuevo de un grupo solo puede sincronizar desde el momento en que fue
admitido. Los mensajes anteriores no forman parte de su historial autorizado.

## Grupos MLS

Los grupos usan MLS (RFC 9420). Añadir, expulsar o revocar un dispositivo crea
una época nueva. Cada dispositivo ocupa una hoja propia; compartir una identidad
humana no significa compartir material de clave entre equipos.

Los grupos tienen propietario y administradores. El propietario puede transferir
su rol o disolver el grupo. El límite es de 16 miembros.

## Red y nodos opcionales

Iroh proporciona QUIC autenticado, descubrimiento, selección de ruta y
atravesado de NAT. pptalk separa dos canales:

- `pptalk/sync/1`: mensajes duraderos, archivos y sincronización;
- `pptalk/call/1`: señales efímeras de llamada.

`pptalk-node` conserva el buzón HTTP legado para pruebas de compatibilidad, pero
no se incluye en los paquetes ni arranca por defecto. No es un servidor de
cuentas, un SFU ni una autoridad de grupos. El relay/NAT traversal en vivo lo
proporciona Iroh.

## Llamadas y multimedia

GStreamer realiza captura, codificación y reproducción nativas. El audio, vídeo
y pantalla viajan como RTP sobre canales de transporte separados de los
mensajes.

Las llamadas de grupo usan una malla entre participantes y tienen un límite duro
de ocho personas. Cada emisor remoto tiene un pipeline de recepción propio, lo
que permite controlar su volumen sin mezclar antes las señales. Los dispositivos
de entrada, salida y cámara se crean desde `GstDevice`; si no se elige uno se usa
el predeterminado del sistema.

El cliente de escritorio usa una única instancia. Una segunda apertura reenvía
el enlace `pptalk://` a la primera, que abre la invitación o trae la ventana al
frente. Si el daemon local termina de forma inesperada, la interfaz lo reinicia
con espera acotada.

## Mapa del código

| Ruta | Responsabilidad |
| --- | --- |
| `apps/desktop` | Interfaz Qt Quick y puente C++ |
| `apps/cli` | Backend JSON Lines y herramientas headless |
| `apps/node` | Buzón cifrado opcional |
| `crates/core` | Identidad, reglas, eventos y cifrado |
| `crates/mls` | Grupos y épocas MLS |
| `crates/network` | Transporte Iroh y canales QUIC |
| `crates/storage` | SQLCipher, outbox y blobs |
| `crates/media` | Captura y calidad GStreamer |
| `crates/protocol` | Tipos CBOR y límites de protocolo |
| `crates/distributed` | Spike Veilid aislado, fuera de la ruta del producto |
