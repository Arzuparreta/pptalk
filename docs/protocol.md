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

Los mensajes directos incluyen un ID estable y, opcionalmente, el ID al que
responden. Edición y eliminación son operaciones firmadas por el autor original.
La confirmación de entrega significa persistencia en el receptor; no existe
confirmación de lectura.

## Archivos

El manifiesto incluye hashes del ciphertext completo y de cada chunk. El
receptor puede aceptar chunks desordenados, deduplicarlos y ensamblarlos de forma
atómica solo cuando todas las verificaciones son correctas.

Si la ruta directa falla durante una transferencia, los chunks restantes quedan
en el outbox local hasta el siguiente intento.

## Llamadas

`CallSignal` expresa:

- invitación con timbre;
- apertura de sala sin timbre;
- entrada y salida;
- rechazo, llamada perdida, retención y reanudación;
- activación de micrófono, cámara o pantalla.

Las señales viajan separadas de los datagramas multimedia. Una llamada de grupo
replica el stream codificado a los peers de una malla. Hay variantes reservadas
para SDP/ICE y ofertas de router, pero la versión actual no anuncia un SFU.

Para cada pareja de endpoints, el menor ID abre una única conexión
`pptalk/media/1`; el otro la acepta. La conexión es dúplex y ambos extremos
mantienen un lector de datagramas. Cada datagrama declara `call_id`, dispositivo
emisor, tipo de medio y secuencia; el receptor los contrasta con el endpoint
QUIC autenticado y con los participantes de la llamada antes de reproducirlos.

## Buzón legado y spike distribuido

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

Esta API se mantiene para compatibilidad y pruebas, pero no se anuncia ni se
configura desde el escritorio. `crates/distributed` contiene la prueba Veilid de
8 MiB máximos y 30 días de caducidad; no se integra mientras un segundo nodo no
pueda publicar, recuperar y cerrar limpiamente de forma repetible.

## Evolución del estado local

Las snapshots MLS llevan una versión propia. El lector admite los formatos
anteriores conocidos y los reescribe al abrirlos. Una versión futura desconocida
se rechaza: nunca debe resolverse una incompatibilidad descartando silenciosamente
claves o creando otra identidad.
