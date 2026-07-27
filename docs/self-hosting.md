# Buzón para mensajes sin conexión

Es un componente opcional. pptalk funciona sin él y no se incluye en los
instaladores, pero el cliente de escritorio sí permite configurarlo desde
**Ajustes → Buzón para mensajes sin conexión**.

## ¿Lo necesitas?

No para hablar cuando ambos dispositivos están conectados. Sirve para un caso
concreto: alguien te escribe, tú estás desconectado, y quien escribe cierra
pptalk antes de que vuelvas. Sin buzón ese mensaje espera en el equipo de quien
lo envió hasta que coincidáis conectados. Con buzón, el mensaje ya cifrado queda
depositado y llega la próxima vez que abras la aplicación.

El nodo:

- no crea cuentas;
- no conoce los contactos o grupos;
- no recibe claves de descifrado;
- no participa en llamadas;
- sí puede observar horarios, tamaños y capacidades de buzón.

Configuras **tu** buzón, no el de tus contactos. Es donde tus amigos dejan lo
que te envían mientras estás fuera. Cuando lo guardas, pptalk avisa a tus
contactos de la dirección; si alguno está desconectado, el aviso espera y le
llega al reconectar.

## Probarlo en local

El entorno de desarrollo ya lo arranca en `127.0.0.1:9464`:

```sh
./scripts/dev.sh start --node-only
curl http://127.0.0.1:9464/healthz
./scripts/dev.sh stop
```

Una respuesta `ok` indica que está listo. Esa dirección solo sirve en la misma
máquina; no es un buzón para amigos a través de Internet.

## Compilar el nodo

```sh
cargo build --locked --release -p pptalk-node
./target/release/pptalk-node \
  --listen 127.0.0.1:9464 \
  --data-dir /var/lib/pptalk-node
```

También existe [apps/node/Dockerfile](../apps/node/Dockerfile). El contenedor
expone el puerto `9464` y usa `/var/lib/pptalk-node` como volumen persistente.

## Publicarlo de forma segura

Mantén el proceso escuchando en localhost y coloca delante un proxy inverso con
HTTPS. Para producción:

1. crea un usuario de sistema sin privilegios;
2. da acceso de escritura únicamente al directorio de datos;
3. usa TLS válido en el proxy;
4. limita peticiones y tamaño de cuerpo;
5. vigila espacio libre y copias del volumen;
6. comprueba periódicamente `GET /healthz`.

Se incluye una unidad de ejemplo en
[packaging/systemd/pptalk-node.service](../packaging/systemd/pptalk-node.service).
Revisa rutas y usuario antes de instalarla.

Los límites actuales son:

- 4 MiB por sobre;
- 256 MiB por capacidad;
- 7 días de retención máxima.

## Usarlo desde la aplicación

Abre **Ajustes**, baja hasta **Buzón para mensajes sin conexión**, escribe la
dirección y pulsa **Guardar buzón**. pptalk comprueba que responde y te lo dice.
Si la dirección es incorrecta el ajuste se guarda igualmente, pero verás un
aviso: hasta que responda, los mensajes seguirán esperando en el equipo de quien
los envía. **Quitar buzón** vuelve al comportamiento anterior.

Solo se acepta HTTPS. HTTP queda permitido únicamente en direcciones loopback
para desarrollo. Dos personas pueden usar el mismo nodo sin compartir capacidad:
cada ruta de entrega utiliza un token distinto.

Para una prueba headless:

```sh
pptalk-cli init --profile alice.json --name Alice \
  --mailbox-url https://pptalk.example
```

## API mínima

```text
GET  /healthz
POST /v1/mailboxes/<64-caracteres-hex>/messages?ttl=86400
GET  /v1/mailboxes/<64-caracteres-hex>/messages?limit=128
```

El cuerpo de `POST` es opaco. `GET` drena el lote solicitado; los clientes se
encargan de autenticidad, descifrado y deduplicación.

Esos 64 caracteres son una capacidad portadora: quien la conozca puede depositar
en esa ruta y, como `GET` vacía el lote, también puede quedarse con lo que haya
sin que llegue a su destinatario. No podría leerlo —va cifrado de extremo a
extremo— pero sí impedir que llegue. La capacidad se deriva del secreto que
compartes con cada contacto y de su dispositivo, así que no aparece en la red ni
la conoce el operador del nodo, pero conviene tenerlo presente al elegir en quién
confías para alojarlo.
