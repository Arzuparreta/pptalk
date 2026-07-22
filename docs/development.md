# Development

## Entorno local

Para el ciclo habitual no hace falta coordinar varios terminales:

```sh
./scripts/dev.sh start              # compila y arranca nodo + escritorio
./scripts/dev.sh status             # PIDs, health y URL local
./scripts/dev.sh logs node -f       # sigue el log del nodo
./scripts/dev.sh logs desktop -f    # sigue el log de Qt y su backend
./scripts/dev.sh restart --no-build # reinicio rápido
./scripts/dev.sh stop               # para solo los procesos gestionados
```

En CI, SSH o una máquina sin sesión gráfica puede arrancarse únicamente el nodo
con `./scripts/dev.sh start --node-only`. Los valores por defecto se pueden
cambiar con `PPTALK_DEV_LISTEN`, `PPTALK_DEV_NODE_URL`,
`PPTALK_DEV_DATA_DIR` y `PPTALK_DEV_STATE_DIR`; la ayuda integrada documenta
los valores concretos. Si se cambia la dirección o el puerto de escucha, la URL
debe apuntar al mismo nodo.

## Rust

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo audit
cargo build -p pptalk-cli
python3 scripts/smoke-e2e.py
```

The Rust integration test uses two real local Iroh endpoints and asserts that
plaintext is absent from wire bytes. The end-to-end smoke additionally drives
real daemon processes through offline outbox recovery, causal MLS catch-up,
silent/ringing call signaling, group attachment transfer, linked-device fan-out
and revocation.

## Native desktop

```sh
cmake -S apps/desktop -B build/desktop -G Ninja -DCMAKE_BUILD_TYPE=Debug
cmake --build build/desktop
QT_QPA_PLATFORM=offscreen timeout 3s ./build/desktop/pptalk-desktop
```

Qt 6.8+ and GStreamer development packages are required. No Node.js toolchain or
browser runtime is used.

## Headless end-to-end workflow

```sh
pptalk-cli init --profile alice.json --name Alice
pptalk-cli init --profile bob.json --name Bob
pptalk-cli invite --profile alice.json
pptalk-cli accept --profile bob.json 'pptalk://contact/v1#...'
pptalk-cli listen --profile alice.json
pptalk-cli send --profile bob.json --contact Alice 'hola'
```

The listener emits newline-delimited JSON for native frontend integration.

For a second device, run `pptalk-cli link-device --profile alice.json --label
Laptop`, transfer the resulting ten-minute capability over a trusted channel,
then use `pptalk-cli import-device --profile laptop.json 'pptalk://device/v1#…'`.
The imported device has a distinct Ed25519 key and Iroh endpoint. MLS leaves are
never cloned: the group owner recognizes the signed device authorization, adds a
fresh leaf and sends a normal MLS Welcome. Revocation removes that leaf and
advances the group epoch.
