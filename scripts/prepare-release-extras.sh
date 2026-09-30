#!/usr/bin/env bash
# Keep the upstream signed MTS-ESP runtime installer in the shared toolkit ZIP.
set -euo pipefail
cd "$(dirname "$0")/.."
mkdir -p dist/release-extras
installer="dist/release-extras/ODDSound-libMTS-1.03.pkg"
curl --fail --location --silent --show-error \
  'https://raw.githubusercontent.com/ODDSound/MTS-ESP/f214739b8832e7f297cb9970d0c0efbf783f1462/libMTS/Mac/x86_64_ARM/libMTSMac_v1.03.pkg' \
  --output "$installer"
echo "5caff5a1f194cddfe5a065cd1b2335fae76ea9d8ff21021071f75b4e7ef0fb85  $installer" | shasum -a 256 -c - >/dev/null
signature="$(pkgutil --check-signature "$installer")"
[[ "$signature" == *'Developer ID Installer: David Gamble (ZTMS6M322L)'* ]] || {
  echo 'Unexpected ODDSound libMTS installer signature' >&2; exit 1
}
