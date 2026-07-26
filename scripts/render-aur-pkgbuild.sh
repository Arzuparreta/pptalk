#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
version="${1:?version is required}"
appimage="${2:?AppImage path is required}"
output="${3:-${repo_root}/build/PKGBUILD}"
version="${version#v}"
checksum="$(sha256sum "${appimage}" | cut -d ' ' -f 1)"
sed -e "s/@VERSION@/${version}/g" -e "s/@SHA256@/${checksum}/g" \
  "${repo_root}/packaging/aur/PKGBUILD.in" > "${output}"
printf '%s\n' "${output}"
