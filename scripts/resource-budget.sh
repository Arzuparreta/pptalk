#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
desktop="${repo_root}/build/desktop/pptalk-desktop"
cli="${repo_root}/target/debug/pptalk-cli"
rss_limit_kb="${PPTALK_IDLE_RSS_KB_LIMIT:-184320}"
cpu_limit="${PPTALK_IDLE_CPU_LIMIT:-2.0}"

[[ -x "${desktop}" ]] || {
  echo "falta ${desktop}; ejecuta ./scripts/dev.sh build" >&2
  exit 1
}
[[ -x "${cli}" ]] || {
  echo "falta ${cli}; ejecuta cargo build --locked -p pptalk-cli" >&2
  exit 1
}

perf_data="$(mktemp -d /tmp/pptalk-resource.XXXXXX)"
cleanup() {
  case "${perf_data}" in
    /tmp/pptalk-resource.*) rm -rf -- "${perf_data}" ;;
  esac
}
trap cleanup EXIT

profile_dir="${perf_data}/pptalk/pptalk"
mkdir -p "${profile_dir}"
"${cli}" init --profile "${profile_dir}/profile.json" --name "Resource check" >/dev/null

QT_QPA_PLATFORM=offscreen \
XDG_DATA_HOME="${perf_data}" \
PPTALK_CLI="${cli}" \
timeout 10s "${desktop}" --minimized &
timeout_pid=$!

sleep 3
desktop_pid="$(pgrep -P "${timeout_pid}" -x pptalk-desktop)"
daemon_pid="$(pgrep -P "${desktop_pid}" -x pptalk-cli)"
desktop_start="$(awk '{print $14+$15}' "/proc/${desktop_pid}/stat")"
daemon_start="$(awk '{print $14+$15}' "/proc/${daemon_pid}/stat")"

sleep 4
desktop_end="$(awk '{print $14+$15}' "/proc/${desktop_pid}/stat")"
daemon_end="$(awk '{print $14+$15}' "/proc/${daemon_pid}/stat")"
ticks="$(getconf CLK_TCK)"
rss_kb="$(ps -o rss= -p "${desktop_pid}" -p "${daemon_pid}" | awk '{sum += $1} END {print sum + 0}')"
cpu_percent="$(awk -v a="${desktop_start}" -v b="${desktop_end}" \
  -v c="${daemon_start}" -v d="${daemon_end}" -v t="${ticks}" \
  'BEGIN { printf "%.2f", (((b-a)+(d-c))/t/4)*100 }')"

set +e
wait "${timeout_pid}"
wait_status=$?
set -e
if [[ "${wait_status}" -ne 0 && "${wait_status}" -ne 124 ]]; then
  echo "pptalk terminó de forma inesperada (${wait_status})" >&2
  exit "${wait_status}"
fi

echo "idle_rss_kb=${rss_kb} limit_kb=${rss_limit_kb}"
echo "idle_cpu_percent=${cpu_percent} limit_percent=${cpu_limit}"

awk -v value="${rss_kb}" -v limit="${rss_limit_kb}" 'BEGIN { exit !(value <= limit) }' ||
  { echo "FAIL: memoria en reposo por encima del presupuesto" >&2; exit 1; }
awk -v value="${cpu_percent}" -v limit="${cpu_limit}" 'BEGIN { exit !(value <= limit) }' ||
  { echo "FAIL: CPU en reposo por encima del presupuesto" >&2; exit 1; }

echo "pptalk resource budget: passed"
