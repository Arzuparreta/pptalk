# Desarrollo

Esta página es para contribuir al código. Para instalar y utilizar la aplicación
consulta [instalación](installation.md) y [guía de uso](user-guide.md).

## Entorno local

```sh
./scripts/dev.sh start              # compila y arranca el escritorio
./scripts/dev.sh start --with-node  # añade el buzón HTTP legado
./scripts/dev.sh status             # muestra PIDs gestionados
./scripts/dev.sh logs desktop -f    # sigue el cliente y su backend
./scripts/dev.sh logs node -f       # sigue el buzón local
./scripts/dev.sh restart --no-build # reinicia sin recompilar
./scripts/dev.sh stop               # para solo procesos de este repo
```

Usa `start --node-only` solo para trabajar en el nodo legado. La ayuda integrada
documenta `PPTALK_DEV_LISTEN`, `PPTALK_DEV_NODE_URL`, `PPTALK_DEV_DATA_DIR`,
`PPTALK_DEV_STATE_DIR` y `CARGO_TARGET_DIR`.

## Comprobaciones antes de entregar un cambio

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
python3 scripts/smoke-e2e.py
```

Para comprobar el presupuesto de reposo del cliente y el daemon en Linux:

```sh
./scripts/resource-budget.sh
```

El gate espera como máximo 180 MiB combinados y un 2 % de un núcleo después del
calentamiento. Se pueden ajustar temporalmente con
`PPTALK_IDLE_RSS_KB_LIMIT` y `PPTALK_IDLE_CPU_LIMIT`.

Para revisar dependencias conocidas como vulnerables:

```sh
cargo audit
```

El smoke test usa procesos reales y cubre reconexión, outbox, sincronización
causal MLS, archivos de grupo, llamadas, segundo dispositivo y revocación.

## Cliente nativo

El script es la ruta habitual. Para compilar Qt de forma aislada:

```sh
cmake -S apps/desktop -B build/desktop -G Ninja -DCMAKE_BUILD_TYPE=Debug
cmake --build build/desktop
QT_QPA_PLATFORM=offscreen timeout 3s ./build/desktop/pptalk-desktop
```

Para revisar una pantalla sin tocar el perfil de uso diario, arranca con una
copia o un perfil temporal y pide una captura al propio `QQuickWindow`:

```sh
PPTALK_PROFILE=/tmp/pptalk-ui/profile.json \
PPTALK_SCREENSHOT_PATH=/tmp/pptalk-ui.png \
QT_QPA_PLATFORM=offscreen ./build/desktop/pptalk-desktop
```

`PPTALK_SCREENSHOT_DELAY_MS` permite aumentar la espera previa a la captura.
`PPTALK_SCREENSHOT_PANEL=settingsDrawer` o `inviteDialog` abre ese panel antes
de capturar.
No uses el único perfil de una persona como fixture visual.

El controlador Qt inicia `pptalk-cli daemon`. Durante desarrollo, define
`PPTALK_CLI` si el binario no está junto al ejecutable de escritorio. No hay
toolchain Node.js ni runtime de navegador.

## Cliente headless

El CLI permite depurar dos identidades sin interfaz:

```sh
pptalk-cli init --profile alice.json --name Alice
pptalk-cli init --profile bob.json --name Bob
pptalk-cli invite --profile alice.json
pptalk-cli accept --profile bob.json 'pptalk://contact/v1#...'
pptalk-cli listen --profile alice.json
pptalk-cli send --profile bob.json --contact Alice 'hola'
```

`daemon` habla JSON Lines por entrada y salida estándar. Es el contrato con el
controlador nativo; cualquier cambio en sus comandos o eventos debe probarse en
ambos lados.

## Publicar una versión

La versión de `[workspace.package]` en `Cargo.toml` es la fuente de verdad. Para
publicar, súbela en un commit a `main`:

```toml
[workspace.package]
version = "0.1.0-alpha.4"
```

Cuando `ci` pasa en `main`, el flujo `release` compara esa versión con los tags
existentes. Si `v0.1.0-alpha.4` no existe todavía, compila el AppImage de Linux
y el instalador de Windows, los somete a sus pruebas de humo y publica la
release creando el tag sobre ese commit. Si el tag ya existe, no hace nada: un
push que no toca la versión solo ejecuta `ci`.

Los tags con guion (`-alpha`, `-beta`) salen marcados como prelanzamiento.

Empujar un tag `v*` a mano sigue funcionando, pero debe coincidir con la versión
del manifiesto; si no coincide, el flujo falla a propósito para que un binario
nunca informe de una versión distinta a aquella con la que se publicó.

## Dónde está cada cosa

- `apps/desktop`: interfaz Qt Quick y controlador C++.
- `apps/cli`: daemon y herramientas headless.
- `apps/node`: buzón opcional.
- `crates/core`: identidades, reglas y cifrado de dominio.
- `crates/mls`: estado de grupos MLS.
- `crates/network`: transporte Iroh.
- `crates/storage`: SQLCipher y outbox.
- `crates/media`: captura y política multimedia GStreamer.
- `crates/protocol`: tipos CBOR versionados.
- `crates/distributed`: spike Veilid aislado de la ruta de producción y del
  workspace distribuido. Veilid 0.5.7 arrastra avisos de seguridad sin solución;
  no lo compiles ni lo empaquetes como parte del cliente.

Las reglas internas para agentes y mantenedores están en [AGENTS.md](../AGENTS.md).
