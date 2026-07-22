# Protocolo v1

Documento para implementadores. El protocolo sigue en desarrollo: hasta la
línea `1.x` no se promete compatibilidad entre versiones arbitrarias.

## Codificación y límites

Los mensajes estructurados usan CBOR y llevan versión. La decodificación limita
la asignación a 4 MiB y rechaza versiones incompatibles antes de aplicar cambios
al dominio.

Los archivos no se introducen completos en un mensaje CBOR. Se dividen en
chunks cifrados y verificados mediante un manifiesto.

## Sincronización duradera

`SyncFrame` tiene tres operaciones principales:

- `Hello`: anuncia identidad, dispositivo y fronteras conocidas.
- `Events`: transmite secuencias que el receptor todavía no cubre.
- `Ack`: confirma persistencia y permite retirar un elemento del outbox.

Un evento es único por su ID y por la combinación de conversación, dispositivo
autor y secuencia. Por eso un reintento directo o desde buzón no duplica el
mensaje visible.

Cada peer vuelve a emitir únicamente eventos que recibió del autor autenticado.
La membresía guarda desde qué momento puede leer cada identidad; un miembro
nuevo no puede pedir historia anterior a su admisión. Otro dispositivo de una
identidad existente conserva ese límite, pero recibe una hoja MLS diferente.

## Archivos

El manifiesto incluye hashes del ciphertext completo y de cada chunk. El
receptor puede aceptar chunks desordenados, deduplicarlos y ensamblarlos de forma
atómica solo cuando todas las verificaciones son correctas.

Si la ruta directa falla durante una transferencia, los chunks restantes pueden
pasar al buzón sin cambiar el contenido cifrado.

## Llamadas

`CallSignal` expresa:

- invitación con timbre;
- apertura de sala sin timbre;
- entrada y salida;
- activación de micrófono, cámara o pantalla.

Las señales viajan separadas de los datagramas multimedia. Una llamada de grupo
replica el stream codificado a los peers de una malla. Hay variantes reservadas
para SDP/ICE y ofertas de router, pero la versión actual no anuncia un SFU.

## Buzón

El acceso es por capacidad. La ruta contiene un token aleatorio de 32 bytes en
hexadecimal:

```text
POST /v1/mailboxes/<capacidad>/messages?ttl=<segundos>
GET  /v1/mailboxes/<capacidad>/messages?limit=<cantidad>
```

`POST` guarda un sobre ya cifrado. `GET` extrae atómicamente un lote. El cliente
deduplica por ID si una respuesta se pierde y el envío se repite.

La capacidad es direccional y no identifica una cuenta global. El nodo impone
tamaño, cuota y caducidad, pero no puede descifrar ni validar el contenido de la
conversación.

## Evolución del estado local

Las snapshots MLS llevan una versión propia. El lector admite los formatos
anteriores conocidos y los reescribe al abrirlos. Una versión futura desconocida
se rechaza: nunca debe resolverse una incompatibilidad descartando silenciosamente
claves o creando otra identidad.
