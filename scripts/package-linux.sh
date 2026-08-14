#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
version="${1:-dev}"
build_dir="${repo_root}/build/package-linux"
stage_dir="${build_dir}/AppDir"
architecture="$(uname -m)"
case "${architecture}" in
  x86_64) appimage_arch="x86_64"; artifact_arch="x86_64" ;;
  aarch64|arm64) appimage_arch="aarch64"; artifact_arch="arm64" ;;
  *) printf 'unsupported architecture: %s\n' "${architecture}" >&2; exit 1 ;;
esac
artifact_arch="${PPTALK_ARTIFACT_ARCH:-${artifact_arch}}"
artifact="${repo_root}/build/pptalk-${version}-linux-${artifact_arch}.AppImage"

rm -rf "${stage_dir}"
cargo build --locked --release --manifest-path "${repo_root}/Cargo.toml" \
  -p pptalk-cli
cmake -S "${repo_root}/apps/desktop" -B "${build_dir}/desktop" -G Ninja \
  -DCMAKE_BUILD_TYPE=Release
cmake --build "${build_dir}/desktop"
cmake --install "${build_dir}/desktop" --prefix "${stage_dir}/usr"
install -Dm755 "${repo_root}/target/release/pptalk-cli" "${stage_dir}/usr/bin/pptalk-cli"
install -Dm644 "${repo_root}/README.md" "${stage_dir}/usr/share/doc/pptalk/README.md"
install -Dm644 "${repo_root}/SECURITY.md" "${stage_dir}/usr/share/doc/pptalk/SECURITY.md"
for document in "${repo_root}"/docs/*.md; do
  install -Dm644 "${document}" "${stage_dir}/usr/share/doc/pptalk/docs/$(basename "${document}")"
done
install -Dm644 "${repo_root}/LICENSE" "${stage_dir}/usr/share/doc/pptalk/LICENSE"
install -Dm644 "${repo_root}/LICENSES/GPL-3.0-or-later.txt" \
  "${stage_dir}/usr/share/doc/pptalk/LICENSES/GPL-3.0-or-later.txt"
install -Dm644 "${repo_root}/LICENSES/MPL-2.0.txt" \
  "${stage_dir}/usr/share/doc/pptalk/LICENSES/MPL-2.0.txt"
gstreamer_plugins="$(pkg-config --variable=pluginsdir gstreamer-1.0)"
install -d "${stage_dir}/usr/lib/gstreamer-1.0"
command -v gst-inspect-1.0 >/dev/null || {
  printf '%s\n' 'gst-inspect-1.0 is required' >&2
  exit 1
}
copy_gstreamer_element() {
  local element="$1"
  local required="${2:-required}"
  local plugin
  plugin="$(
    LC_ALL=C GST_PLUGIN_SYSTEM_PATH_1_0="${gstreamer_plugins}" \
      gst-inspect-1.0 "${element}" 2>/dev/null |
      sed -n 's/^[[:space:]]*Filename[[:space:]]*//p' |
      head -n 1
  )"
  if [[ -z "${plugin}" || ! -f "${plugin}" ]]; then
    if [[ "${required}" == required ]]; then
      printf 'required GStreamer element is unavailable: %s\n' "${element}" >&2
      exit 1
    fi
    return 1
  fi
  install -Dm755 "${plugin}" "${stage_dir}/usr/lib/gstreamer-1.0/$(basename "${plugin}")"
}
for element in \
  appsrc appsink queue audioconvert audioresample opusenc opusdec \
  rtpopuspay rtpopusdepay rtpjitterbuffer volume videoconvert videoscale \
  videorate h264parse rtph264pay rtph264depay decodebin autoaudiosink \
  autovideosink; do
  copy_gstreamer_element "${element}"
done
copy_at_least_one() {
  local description="$1"
  shift
  local found=0
  local element
  for element in "$@"; do
    if copy_gstreamer_element "${element}" optional; then found=1; fi
  done
  if [[ "${found}" -eq 0 ]]; then
    printf 'no supported GStreamer %s plugin is available\n' "${description}" >&2
    exit 1
  fi
}
copy_at_least_one "audio capture" pulsesrc pipewiresrc alsasrc
copy_at_least_one "camera capture" v4l2src pipewiresrc
copy_at_least_one "screen capture" pipewiresrc ximagesrc
copy_at_least_one "H.264 encoder" openh264enc x264enc
copy_at_least_one "H.264 decoder" openh264dec avdec_h264
for element in pulsesink pipewiresink alsasink waylandsink ximagesink xvimagesink; do
  copy_gstreamer_element "${element}" optional || true
done

# The GStreamer plugins above pull runtime libraries (pipewire, spa, X11,
# xcb...) that linuxdeploy cannot see because nothing it scans links them.
# Resolve the missing closure against the host so the AppImage is
# self-contained instead of crashing on minimal systems.
bundle_library_closure() {
  local stage="$1"
  local lib_dir="$stage/usr/lib"
  local lib_dir_alt="$stage/usr/lib/x86_64-linux-gnu"
  local excluded='^(libc\.so|libm\.so|ld-linux|libpthread|libdl|librt|libresolv|libgcc_s|libanl|libutil)'
  local pass lib missing path
  for pass in 1 2 3; do
    local copied=0
    while IFS= read -r missing; do
      lib="${missing##*/}"
      [[ -e "${lib_dir}/${lib}" || -e "${lib_dir_alt}/${lib}" ]] && continue
      [[ "${lib}" =~ ${excluded} ]] && continue
      path="$(ldconfig -p | awk -v lib="${lib}" '$1 == lib { print $NF; exit }')"
      if [[ -z "${path}" || ! -f "${path}" ]]; then
        printf 'cannot resolve bundled library dependency: %s\n' "${lib}" >&2
        exit 1
      fi
      install -Dm755 "${path}" "${lib_dir}/${lib}"
      copied=1
      printf 'bundled %s\n' "${lib}"
    done < <(
      find "${stage}/usr/lib" -name '*.so*' -type f | while read -r candidate; do
        LD_LIBRARY_PATH="${lib_dir}:${lib_dir_alt}" \
          ldd "${candidate}" 2>/dev/null | awk '$2 == "=>" && $3 == "not" { print $1 }'
      done | sort -u
    )
    [[ "${copied}" -eq 0 ]] && break
  done
}
bundle_library_closure "${stage_dir}"
gstreamer_scanner="$(pkg-config --variable=pluginscannerdir gstreamer-1.0)/gst-plugin-scanner"
install -Dm755 "${gstreamer_scanner}" \
  "${stage_dir}/usr/libexec/gstreamer-1.0/gst-plugin-scanner"
command -v linuxdeploy >/dev/null || { printf '%s\n' 'linuxdeploy is required' >&2; exit 1; }
command -v linuxdeploy-plugin-qt >/dev/null || { printf '%s\n' 'linuxdeploy-plugin-qt is required' >&2; exit 1; }
export ARCH="${appimage_arch}"
export OUTPUT="${artifact}"
export UPD_INFO="gh-releases-zsync|Arzuparreta|pptalk|latest|pptalk-*-linux-${artifact_arch}.AppImage.zsync"
export QML_SOURCES_PATHS="${repo_root}/apps/desktop/qml"
linuxdeploy \
  --appdir "${stage_dir}" \
  --desktop-file "${repo_root}/packaging/linux/pptalk.desktop" \
  --icon-file "${repo_root}/packaging/linux/pptalk.svg" \
  --custom-apprun "${repo_root}/packaging/linux/AppRun" \
  --plugin qt \
  --output appimage
printf '%s\n' "${artifact}"
