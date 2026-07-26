# Buzón HTTP legado para desarrollo

El cliente de escritorio ya no ofrece este componente como opción de producto.
Se conserva para pruebas de compatibilidad y no se incluye en los instaladores.

## ¿Lo necesitas?

No para hablar cuando ambos dispositivos están conectados. El buzón solo ayuda a
entregar mensajes ya cifrados mientras alguien está desconectado.

El nodo:

- no crea cuentas;
- no conoce los contactos o grupos;
- no recibe claves de descifrado;
- no participa en llamadas;
- sí puede observar horarios, tamaños y capacidades de buzón.

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

## Usarlo con el cliente headless legado

El escritorio no expone este ajuste. Para una prueba headless:

```sh
pptalk-cli init --profile alice.json --name Alice \
  --mailbox-url https://pptalk.example
```

HTTP se acepta únicamente en direcciones loopback para desarrollo. Dos personas
pueden usar el mismo nodo sin compartir capacidad: cada ruta de entrega utiliza
un token distinto.

## API mínima

```text
GET  /healthz
POST /v1/mailboxes/<64-caracteres-hex>/messages?ttl=86400
GET  /v1/mailboxes/<64-caracteres-hex>/messages?limit=128
```

El cuerpo de `POST` es opaco. `GET` drena el lote solicitado; los clientes se
encargan de autenticidad, descifrado y deduplicación.
