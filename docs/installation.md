# Instalar y abrir pptalk

pptalk aún se distribuye desde el código fuente. Esta guía deja la aplicación
lista para probarla; no necesitas montar un servidor ni crear una cuenta.

## Linux

Instala estas herramientas con el gestor de paquetes de tu distribución:

- Rust 1.91 o posterior, mediante `rustup` o el paquete de tu distribución.
- CMake 3.24 o posterior y Ninja.
- Qt 6.8 o posterior con Core, Gui, Qml, Quick y Quick Controls 2.
- GStreamer 1.24 o posterior con complementos de captura, RTP, Opus y H.264.
- Bash y `curl`.

Después clona el repositorio y arranca el entorno:

```sh
git clone https://github.com/Arzuparreta/pptalk.git
cd pptalk
./scripts/dev.sh start
```

El script:

1. compila el cliente y sus componentes;
2. abre el cliente nativo Qt;
3. crea o migra el perfil local sin borrar la identidad existente.

No usa Electron, Chromium, Node.js ni un servidor web para la interfaz.

En los siguientes arranques puedes evitar la recompilación:

```sh
./scripts/dev.sh start --no-build
```

Usa `./scripts/dev.sh restart` después de cambiar código y
`./scripts/dev.sh stop` para cerrar los procesos que inició el script.

## Windows

Necesitas Rust, CMake, Ninja, Qt 6.8+ y GStreamer 1.24+ disponibles en `PATH`.
Desde PowerShell:

```powershell
cargo build --locked -p pptalk-cli
cmake -S apps/desktop -B build/desktop -G Ninja -DCMAKE_BUILD_TYPE=Debug
cmake --build build/desktop
$env:PPTALK_CLI = (Resolve-Path target/debug/pptalk-cli.exe)
./build/desktop/pptalk-desktop.exe
```

Según el generador de CMake, el ejecutable puede quedar en una subcarpeta como
`build/desktop/Debug/`.

Las etiquetas de versión generan un instalador `.exe` para Windows y AppImage
para Linux x86_64 y arm64. El paquete `pptalk-bin` de AUR usa la AppImage x86_64;
hasta que exista una versión publicada, compila desde el repositorio.

## Comprobar la instalación

```sh
./scripts/dev.sh doctor
```

El diagnóstico debe mostrar transporte Iroh, base SQLCipher, multimedia
GStreamer y escritorio Qt Quick. Para ver el estado y los logs:

```sh
./scripts/dev.sh status
./scripts/dev.sh logs -f
```

## Datos locales

En Linux, Qt guarda normalmente el perfil en:

```text
~/.local/share/pptalk/pptalk/
```

El directorio contiene la identidad del dispositivo y la base cifrada. No lo
borres para solucionar un error y no lo compartas con nadie. Haz una copia
segura si el perfil te importa. Los logs y PIDs de desarrollo quedan en
`build/dev/`; esa carpeta sí es desechable.

## Desinstalar

Primero para el entorno:

```sh
./scripts/dev.sh stop
```

Después puedes borrar el clon del repositorio. El perfil personal se conserva
en el directorio de datos indicado arriba; elimínalo únicamente si quieres
destruir también esa identidad y su historial.
