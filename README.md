# pptalk

Private, local-first communication for friends: encrypted chat, calls, video and
screen sharing over direct peer-to-peer connections with replaceable relays.

This repository contains the native desktop client, the reusable Rust core and
the optional self-hosted node. No central account or server owns an identity,
conversation or message history.

## What works

- native Qt Quick desktop with direct/group chat, encrypted files, invitations and call controls
- signed device identities, live multi-device fan-out/revocation and creator-owned groups
- RFC 9420 MLS group epochs, creator-controlled membership and SQLCipher history
- direct authenticated QUIC plus automatic encrypted relay fallback through Iroh
- durable local outbox plus causal group catch-up after a peer reconnects
- signed one-use contact links and a usable headless encrypted messaging flow
- native GStreamer RTP capture/playback for voice, camera and screen sources
- separate call signaling and P2P mesh fan-out (optimized for eight, without a hard cap)
- optional durable capability mailbox with TTL and quota enforcement
- Linux x86_64/ARM64 and Windows build coverage

The protocol is versioned from the first commit. Compatibility is not promised
until the `1.x` protocol line. This is a working developer release, not an
independently audited security product; read the [threat model](docs/threat-model.md).

## Build

Requirements:

- Rust 1.91 or newer
- CMake and Ninja
- Qt 6.8 or newer (`Core`, `Gui`, `Qml`, `Quick`, `QuickControls2`)
- GStreamer 1.24 or newer with RTP, Opus, H.264 and native capture/sink plugins

```sh
cargo test --workspace
cargo run -p pptalk-cli -- doctor
cmake -S apps/desktop -B build/desktop -G Ninja
cmake --build build/desktop
```

## Desarrollo local

El entorno completo se gestiona con un único script. Compila el CLI, el nodo
mailbox y la aplicación Qt; arranca el nodo solo en localhost, comprueba su
salud y abre el cliente nativo con la configuración correcta:

```sh
./scripts/dev.sh start
./scripts/dev.sh status
./scripts/dev.sh logs -f
./scripts/dev.sh stop
```

Los procesos quedan en segundo plano. Sus PIDs, logs y datos locales se guardan
en `build/dev/` (ignorado por Git), y `stop` valida que cada PID pertenezca a un
binario de este repositorio antes de terminarlo. Para trabajar únicamente con
el nodo, usa `./scripts/dev.sh start --node-only`; para reiniciar sin recompilar,
`./scripts/dev.sh restart --no-build`. Ejecuta `./scripts/dev.sh help` para ver
todas las opciones y variables de configuración.

See [`docs/architecture.md`](docs/architecture.md) and
[`docs/self-hosting.md`](docs/self-hosting.md) for the protocol and deployment
model. Security reports and the supported disclosure channel are in
[`SECURITY.md`](SECURITY.md).

## Try two peers

```sh
cargo run -p pptalk-cli -- init --profile /tmp/alice.json --name Alice
cargo run -p pptalk-cli -- init --profile /tmp/bob.json --name Bob
cargo run -p pptalk-cli -- invite --profile /tmp/alice.json
# accept the printed link on Bob, keep Alice listening, then send:
cargo run -p pptalk-cli -- listen --profile /tmp/alice.json
cargo run -p pptalk-cli -- send --profile /tmp/bob.json --contact Alice 'hola'
```

For offline delivery, initialize with `--mailbox-url https://your-node.example`
or configure the URL from desktop settings. Device links are ten-minute
capabilities generated in Settings; on a fresh desktop launch they can be
imported with `PPTALK_DEVICE_LINK='pptalk://device/v1#…'`.

## Licensing

- Desktop client and shared client code: `GPL-3.0-or-later`
- Network services in `apps/node`: `AGPL-3.0-or-later`
- Documentation and packaging: `GPL-3.0-or-later`
