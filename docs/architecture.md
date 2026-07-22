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
- el creador de un grupo controla quién entra y sale;
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
firmada por uno ya válido. No se clona una identidad MLS. Al revocarlo, deja de
recibir envíos y los grupos controlados avanzan a claves que ya no lo incluyen.

Las invitaciones de contacto contienen una capacidad de un solo uso, una prueba
firmada de dirección y una caducidad. La interfaz actual confía en el canal por
el que se comparte la invitación; aún falta una pantalla de comparación de
huellas para verificaciones de mayor riesgo.

## Conversaciones y sincronización

Cada autor mantiene una secuencia monotónica por dispositivo. Al reconectar, los
peers comparan sus fronteras causales y transmiten únicamente los eventos que
faltan.

Si ninguna ruta está disponible, el sobre cifrado permanece en el outbox del
emisor. Si el destinatario anunció un buzón, el emisor también puede depositarlo
allí. El nodo nunca recibe el contenido sin cifrar.

Un miembro nuevo de un grupo solo puede sincronizar desde el momento en que fue
admitido. Los mensajes anteriores no forman parte de su historial autorizado.

## Grupos MLS

Los grupos usan MLS (RFC 9420). Añadir, expulsar o revocar un dispositivo crea
una época nueva. Cada dispositivo ocupa una hoja propia; compartir una identidad
humana no significa compartir material de clave entre equipos.

En la implementación actual, el creador es el único administrador del grupo.
Esto simplifica el consenso P2P inicial, aunque no pretende ser el modelo final
para comunidades grandes.

## Red y nodos opcionales

Iroh proporciona QUIC autenticado, descubrimiento, selección de ruta y
atravesado de NAT. pptalk separa dos canales:

- `pptalk/sync/1`: mensajes duraderos, archivos y sincronización;
- `pptalk/call/1`: señales efímeras de llamada.

`pptalk-node` implementa actualmente un buzón HTTP de almacenamiento y reenvío.
No es un servidor de cuentas, un historial central, un SFU ni una autoridad de
grupos. El relay/NAT traversal en vivo lo proporciona Iroh.

## Llamadas y multimedia

GStreamer realiza captura, codificación y reproducción nativas. El audio, vídeo
y pantalla viajan como RTP sobre canales de transporte separados de los
mensajes.

Las llamadas de grupo usan una malla entre participantes. El objetivo de
optimización actual son ocho personas; no hay un límite duro de protocolo, pero
una malla no escala como un SFU.

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
