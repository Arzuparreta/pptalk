#!/usr/bin/env bash

set -Eeuo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd -- "$SCRIPT_DIR/.." && pwd)"

STATE_DIR="${PPTALK_DEV_STATE_DIR:-$REPO_ROOT/build/dev}"
DATA_DIR="${PPTALK_DEV_DATA_DIR:-$STATE_DIR/data/node}"
NODE_LISTEN="${PPTALK_DEV_LISTEN:-127.0.0.1:9464}"
NODE_URL="${PPTALK_DEV_NODE_URL:-http://127.0.0.1:9464}"

RUST_TARGET_DIR="${CARGO_TARGET_DIR:-$REPO_ROOT/target}"
CLI_BIN="$RUST_TARGET_DIR/debug/pptalk-cli"
NODE_BIN="$RUST_TARGET_DIR/debug/pptalk-node"
DESKTOP_BUILD_DIR="$REPO_ROOT/build/desktop"
DESKTOP_BIN="$DESKTOP_BUILD_DIR/pptalk-desktop"

NODE_PID_FILE="$STATE_DIR/node.pid"
DESKTOP_PID_FILE="$STATE_DIR/desktop.pid"
NODE_LOG="$STATE_DIR/node.log"
DESKTOP_LOG="$STATE_DIR/desktop.log"

STARTED_NODE=false
STARTED_DESKTOP=false
ROLLBACK_ON_EXIT=false

usage() {
    cat <<'EOF'
Uso: ./scripts/dev.sh <comando> [opciones]

Comandos:
  start [--node-only] [--no-build]  Compila y arranca nodo + cliente nativo
  stop                              Para los procesos gestionados por el script
  restart [opciones de start]       Reinicia el entorno
  status                            Muestra procesos, URL y estado de salud
  logs [node|desktop] [-f]          Muestra los logs (con -f, los sigue)
  build [--node-only]               Compila los binarios de desarrollo
  doctor                            Ejecuta el diagnóstico del CLI
  help                              Muestra esta ayuda

Variables opcionales:
  PPTALK_DEV_LISTEN       Dirección del nodo (por defecto 127.0.0.1:9464)
  PPTALK_DEV_NODE_URL     URL que usa el cliente (por defecto http://127.0.0.1:9464)
  PPTALK_DEV_DATA_DIR     Datos persistentes del nodo
  PPTALK_DEV_STATE_DIR    PIDs y logs (por defecto build/dev)
  CARGO_TARGET_DIR        Directorio target de Cargo
EOF
}

die() {
    printf 'error: %s\n' "$*" >&2
    exit 1
}

require_command() {
    command -v "$1" >/dev/null 2>&1 || die "falta '$1' en PATH"
}

pid_from_file() {
    local file="$1"
    [[ -f "$file" ]] || return 1
    local pid
    pid="$(<"$file")"
    [[ "$pid" =~ ^[0-9]+$ ]] || return 1
    printf '%s\n' "$pid"
}

pid_is_owned() {
    local pid="$1"
    local expected="$2"
    kill -0 "$pid" 2>/dev/null || return 1

    if [[ -e "/proc/$pid/exe" ]]; then
        [[ "$(readlink -f -- "/proc/$pid/exe")" == "$(readlink -f -- "$expected")" ]]
        return
    fi

    local command_line
    command_line="$(ps -p "$pid" -o command= 2>/dev/null || true)"
    [[ "$command_line" == "$expected" || "$command_line" == "$expected "* ]]
}

managed_pid() {
    local file="$1"
    local expected="$2"
    local pid
    pid="$(pid_from_file "$file" 2>/dev/null || true)"
    [[ -n "$pid" ]] && pid_is_owned "$pid" "$expected" || return 1
    printf '%s\n' "$pid"
}

build_rust() {
    require_command cargo
    printf 'Compilando CLI y nodo...\n'
    (cd "$REPO_ROOT" && cargo build --locked -p pptalk-cli -p pptalk-node)
}

build_desktop() {
    require_command cmake
    require_command ninja
    printf 'Configurando cliente Qt...\n'
    cmake -S "$REPO_ROOT/apps/desktop" -B "$DESKTOP_BUILD_DIR" \
        -G Ninja -DCMAKE_BUILD_TYPE=Debug
    printf 'Compilando cliente Qt...\n'
    cmake --build "$DESKTOP_BUILD_DIR"
}

build_all() {
    local node_only="${1:-false}"
    build_rust
    if [[ "$node_only" != true ]]; then
        build_desktop
    fi
}

wait_for_node() {
    local pid="$1"
    local attempt
    for attempt in {1..80}; do
        if ! kill -0 "$pid" 2>/dev/null; then
            return 1
        fi
        if curl --silent --show-error --fail --max-time 1 "$NODE_URL/healthz" >/dev/null 2>&1; then
            pid_is_owned "$pid" "$NODE_BIN"
            return
        fi
        sleep 0.25
    done
    return 1
}

start_node() {
    local pid
    if pid="$(managed_pid "$NODE_PID_FILE" "$NODE_BIN" 2>/dev/null)"; then
        printf 'Nodo ya activo (PID %s, %s).\n' "$pid" "$NODE_URL"
        return
    fi
    rm -f -- "$NODE_PID_FILE"

    if curl --silent --fail --max-time 1 "$NODE_URL/healthz" >/dev/null 2>&1; then
        die "$NODE_URL ya responde, pero no pertenece a este entorno gestionado"
    fi

    mkdir -p -- "$STATE_DIR" "$DATA_DIR"
    : >"$NODE_LOG"
    nohup "$NODE_BIN" --listen "$NODE_LISTEN" --data-dir "$DATA_DIR" \
        >>"$NODE_LOG" 2>&1 &
    pid=$!
    printf '%s\n' "$pid" >"$NODE_PID_FILE"

    if ! wait_for_node "$pid"; then
        printf 'El nodo no llegó a estar listo. Últimas líneas:\n' >&2
        tail -n 30 "$NODE_LOG" >&2 || true
        stop_process "Nodo" "$NODE_PID_FILE" "$NODE_BIN" >&2
        die "falló el arranque del nodo"
    fi
    STARTED_NODE=true
    printf 'Nodo listo (PID %s): %s\n' "$pid" "$NODE_URL"
}

start_desktop() {
    local pid
    if pid="$(managed_pid "$DESKTOP_PID_FILE" "$DESKTOP_BIN" 2>/dev/null)"; then
        printf 'Cliente ya activo (PID %s).\n' "$pid"
        return
    fi
    rm -f -- "$DESKTOP_PID_FILE"

    mkdir -p -- "$STATE_DIR"
    : >"$DESKTOP_LOG"
    nohup env \
        PPTALK_CLI="$CLI_BIN" \
        PPTALK_MAILBOX_URL="$NODE_URL" \
        "$DESKTOP_BIN" >>"$DESKTOP_LOG" 2>&1 &
    pid=$!
    printf '%s\n' "$pid" >"$DESKTOP_PID_FILE"
    sleep 1

    if ! pid_is_owned "$pid" "$DESKTOP_BIN"; then
        rm -f -- "$DESKTOP_PID_FILE"
        printf 'El cliente terminó durante el arranque. Últimas líneas:\n' >&2
        tail -n 30 "$DESKTOP_LOG" >&2 || true
        die "falló el arranque del cliente"
    fi
    STARTED_DESKTOP=true
    printf 'Cliente nativo listo (PID %s).\n' "$pid"
}

stop_process() {
    local label="$1"
    local file="$2"
    local expected="$3"
    local pid

    pid="$(pid_from_file "$file" 2>/dev/null || true)"
    if [[ -z "$pid" ]]; then
        printf '%s no estaba activo.\n' "$label"
        rm -f -- "$file"
        return
    fi
    if ! pid_is_owned "$pid" "$expected"; then
        printf '%s tenía un PID obsoleto; no se ha terminado ningún proceso.\n' "$label"
        rm -f -- "$file"
        return
    fi

    kill -TERM "$pid"
    local attempt
    for attempt in {1..40}; do
        if ! kill -0 "$pid" 2>/dev/null; then
            rm -f -- "$file"
            printf '%s parado.\n' "$label"
            return
        fi
        sleep 0.25
    done

    if pid_is_owned "$pid" "$expected"; then
        kill -KILL "$pid"
    fi
    rm -f -- "$file"
    printf '%s forzado a parar tras 10 segundos.\n' "$label"
}

stop_all() {
    stop_process "Cliente" "$DESKTOP_PID_FILE" "$DESKTOP_BIN"
    stop_process "Nodo" "$NODE_PID_FILE" "$NODE_BIN"
}

rollback_start() {
    local status=$?
    trap - EXIT
    if [[ "$ROLLBACK_ON_EXIT" == true && "$status" -ne 0 ]]; then
        printf 'Deshaciendo el arranque incompleto...\n' >&2
        if [[ "$STARTED_DESKTOP" == true ]]; then
            stop_process "Cliente" "$DESKTOP_PID_FILE" "$DESKTOP_BIN" >&2
        fi
        if [[ "$STARTED_NODE" == true ]]; then
            stop_process "Nodo" "$NODE_PID_FILE" "$NODE_BIN" >&2
        fi
    fi
    exit "$status"
}

show_status() {
    local found=false
    local pid
    if pid="$(managed_pid "$NODE_PID_FILE" "$NODE_BIN" 2>/dev/null)"; then
        local health="no disponible"
        if curl --silent --fail --max-time 1 "$NODE_URL/healthz" >/dev/null 2>&1; then
            health="ok"
        fi
        printf 'Nodo:    activo (PID %s, health %s, %s)\n' "$pid" "$health" "$NODE_URL"
        found=true
    else
        printf 'Nodo:    parado\n'
    fi
    if pid="$(managed_pid "$DESKTOP_PID_FILE" "$DESKTOP_BIN" 2>/dev/null)"; then
        printf 'Cliente: activo (PID %s)\n' "$pid"
        found=true
    else
        printf 'Cliente: parado\n'
    fi
    printf 'Logs:    %s\n' "$STATE_DIR"
    [[ "$found" == true ]]
}

show_logs() {
    local service="${1:-all}"
    local follow="${2:-false}"
    local -a files=()
    case "$service" in
        all) files=("$NODE_LOG" "$DESKTOP_LOG") ;;
        node) files=("$NODE_LOG") ;;
        desktop) files=("$DESKTOP_LOG") ;;
        *) die "servicio de logs desconocido: $service" ;;
    esac

    local -a existing=()
    local file
    for file in "${files[@]}"; do
        [[ -f "$file" ]] && existing+=("$file")
    done
    ((${#existing[@]} > 0)) || die "todavía no hay logs en $STATE_DIR"
    if [[ "$follow" == true ]]; then
        tail -n 80 -F "${existing[@]}"
    else
        tail -n 80 "${existing[@]}"
    fi
}

parse_mode_flags() {
    NODE_ONLY=false
    NO_BUILD=false
    while (($#)); do
        case "$1" in
            --node-only) NODE_ONLY=true ;;
            --no-build) NO_BUILD=true ;;
            *) die "opción desconocida: $1" ;;
        esac
        shift
    done
}

command_start() {
    parse_mode_flags "$@"
    STARTED_NODE=false
    STARTED_DESKTOP=false
    ROLLBACK_ON_EXIT=true
    trap rollback_start EXIT
    require_command curl
    if [[ "$NO_BUILD" != true ]]; then
        build_all "$NODE_ONLY"
    else
        [[ -x "$NODE_BIN" ]] || die "no existe $NODE_BIN; ejecuta start sin --no-build"
        if [[ "$NODE_ONLY" != true ]]; then
            [[ -x "$CLI_BIN" ]] || die "no existe $CLI_BIN; ejecuta start sin --no-build"
            [[ -x "$DESKTOP_BIN" ]] || die "no existe $DESKTOP_BIN; ejecuta start sin --no-build"
        fi
    fi
    start_node
    if [[ "$NODE_ONLY" != true ]]; then
        start_desktop
    fi
    ROLLBACK_ON_EXIT=false
    trap - EXIT
    printf '\nEntorno pptalk listo. Usa %s status, logs o stop.\n' "$0"
}

command_build() {
    parse_mode_flags "$@"
    [[ "$NO_BUILD" == false ]] || die "--no-build no es válido con build"
    build_all "$NODE_ONLY"
}

command_logs() {
    local service="all"
    local follow=false
    while (($#)); do
        case "$1" in
            node|desktop) service="$1" ;;
            -f|--follow) follow=true ;;
            *) die "opción desconocida: $1" ;;
        esac
        shift
    done
    show_logs "$service" "$follow"
}

main() {
    local command="${1:-help}"
    if (($#)); then shift; fi
    case "$command" in
        start) command_start "$@" ;;
        stop) (($# == 0)) || die "stop no acepta opciones"; stop_all ;;
        restart) stop_all; command_start "$@" ;;
        status) (($# == 0)) || die "status no acepta opciones"; show_status ;;
        logs) command_logs "$@" ;;
        build) command_build "$@" ;;
        doctor)
            (($# == 0)) || die "doctor no acepta opciones"
            build_rust
            "$CLI_BIN" doctor
            ;;
        help|-h|--help) usage ;;
        *) usage >&2; die "comando desconocido: $command" ;;
    esac
}

main "$@"
