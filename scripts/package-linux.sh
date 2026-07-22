#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
version="${1:-dev}"
build_dir="${repo_root}/build/package-linux"
stage_dir="${build_dir}/stage"
artifact="${repo_root}/build/pptalk-${version}-linux-$(uname -m).tar.zst"

rm -rf "${stage_dir}"
cargo build --locked --release --manifest-path "${repo_root}/Cargo.toml" \
  -p pptalk-cli -p pptalk-node
cmake -S "${repo_root}/apps/desktop" -B "${build_dir}/desktop" -G Ninja \
  -DCMAKE_BUILD_TYPE=Release
cmake --build "${build_dir}/desktop"
cmake --install "${build_dir}/desktop" --prefix "${stage_dir}/usr"
install -Dm755 "${repo_root}/target/release/pptalk-cli" "${stage_dir}/usr/bin/pptalk-cli"
install -Dm755 "${repo_root}/target/release/pptalk-node" "${stage_dir}/usr/bin/pptalk-node"
install -Dm644 "${repo_root}/packaging/systemd/pptalk-node.service" \
  "${stage_dir}/usr/lib/systemd/system/pptalk-node.service"
install -Dm644 "${repo_root}/README.md" "${stage_dir}/usr/share/doc/pptalk/README.md"
install -Dm644 "${repo_root}/SECURITY.md" "${stage_dir}/usr/share/doc/pptalk/SECURITY.md"
for document in "${repo_root}"/docs/*.md; do
  install -Dm644 "${document}" "${stage_dir}/usr/share/doc/pptalk/docs/$(basename "${document}")"
done
install -Dm644 "${repo_root}/LICENSE" "${stage_dir}/usr/share/doc/pptalk/LICENSE"
install -Dm644 "${repo_root}/LICENSES/GPL-3.0-or-later.txt" \
  "${stage_dir}/usr/share/doc/pptalk/LICENSES/GPL-3.0-or-later.txt"
install -Dm644 "${repo_root}/LICENSES/AGPL-3.0-or-later.txt" \
  "${stage_dir}/usr/share/doc/pptalk/LICENSES/AGPL-3.0-or-later.txt"
tar --zstd -C "${stage_dir}" -cf "${artifact}" .
printf '%s\n' "${artifact}"
