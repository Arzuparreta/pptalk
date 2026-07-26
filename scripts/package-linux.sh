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
cp -a "${gstreamer_plugins}"/*.so "${stage_dir}/usr/lib/gstreamer-1.0/"
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
