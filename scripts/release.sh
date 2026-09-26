#!/usr/bin/env bash
# Build a locally signed, notarized, and audited macOS VST3 release.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${repo_root}"

usage() {
  cat <<'EOF'
Usage: scripts/release.sh [--check | --package-only]

--check         Check release prerequisites without building or contacting Apple.
--package-only  Build, Developer ID sign, notarize, staple, and audit a VST3 ZIP.

Requires VST3_SDK_DIR and either APPLE_NOTARY_KEYCHAIN_PROFILE or all of
APPLE_NOTARY_KEY_PATH, APPLE_NOTARY_KEY_ID, and APPLE_NOTARY_ISSUER_ID.
APPLE_CODESIGN_IDENTITY may select an installed Developer ID Application identity.
The source must be committed and clean. This script never publishes or installs.
EOF
}

mode="package-only"
if [[ $# -gt 1 ]]; then usage >&2; exit 2; fi
if [[ $# -eq 1 ]]; then
  case "$1" in
    --check) mode="check" ;;
    --package-only) ;;
    -h|--help) usage; exit 0 ;;
    *) usage >&2; exit 2 ;;
  esac
fi

problems=()
[[ "$(uname -s)" == Darwin ]] || problems+=("macOS is required")
for tool in cargo codesign security xcrun ditto lipo python3 curl shasum pkgutil; do
  command -v "${tool}" >/dev/null || problems+=("missing tool: ${tool}")
done
[[ -d "${VST3_SDK_DIR:-}/pluginterfaces" ]] || problems+=("VST3_SDK_DIR must point to a VST3 SDK checkout")
source_sha="$(git rev-parse --verify HEAD 2>/dev/null)" || problems+=("release source needs an initial commit")
[[ -z "$(git status --porcelain --untracked-files=all)" ]] || problems+=("release source has staged or unstaged changes; commit the intended source first")
version="$(sed -n '/^\[package\]/,/^\[/ { s/^version[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p; }' Cargo.toml | head -n 1)"
[[ "${version}" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || problems+=("a stable X.Y.Z Cargo.toml version is required")

identity="${APPLE_CODESIGN_IDENTITY:-}"
if [[ -z "${identity}" ]] && command -v security >/dev/null; then
  identity="$(security find-identity -v -p codesigning | sed -n 's/.*"\(Developer ID Application:.*\)".*/\1/p' | head -n 1)"
fi
if [[ "${identity}" != Developer\ ID\ Application:* ]]; then
  problems+=("an installed Developer ID Application identity is required")
elif ! security find-identity -v -p codesigning | grep -Fq "\"${identity}\""; then
  problems+=("Developer ID Application identity is unavailable")
fi

notary_auth=()
if [[ -n "${APPLE_NOTARY_KEYCHAIN_PROFILE:-}" ]]; then
  if [[ "${APPLE_NOTARY_KEYCHAIN_PROFILE}" == your-notarytool-profile ]]; then
    problems+=("your-notarytool-profile is an example; save a real notarytool profile and use its name")
  else
    notary_auth=(--keychain-profile "${APPLE_NOTARY_KEYCHAIN_PROFILE}")
  fi
else
  for name in APPLE_NOTARY_KEY_PATH APPLE_NOTARY_KEY_ID APPLE_NOTARY_ISSUER_ID; do
    [[ -n "${!name:-}" ]] || problems+=("missing ${name} or APPLE_NOTARY_KEYCHAIN_PROFILE")
  done
  if [[ -n "${APPLE_NOTARY_KEY_PATH:-}" ]]; then
    [[ -f "${APPLE_NOTARY_KEY_PATH}" && ! -L "${APPLE_NOTARY_KEY_PATH}" ]] || problems+=("APPLE_NOTARY_KEY_PATH must be a regular file")
  fi
  if [[ -n "${APPLE_NOTARY_KEY_ID:-}" ]]; then
    [[ "${APPLE_NOTARY_KEY_ID}" =~ ^[A-Z0-9]{10}$ ]] || problems+=("invalid APPLE_NOTARY_KEY_ID")
  fi
  if [[ -n "${APPLE_NOTARY_ISSUER_ID:-}" ]]; then
    [[ "${APPLE_NOTARY_ISSUER_ID}" =~ ^[A-Fa-f0-9-]{36}$ ]] || problems+=("invalid APPLE_NOTARY_ISSUER_ID")
  fi
  if [[ -n "${APPLE_NOTARY_KEY_PATH:-}" && -n "${APPLE_NOTARY_KEY_ID:-}" && -n "${APPLE_NOTARY_ISSUER_ID:-}" ]]; then
    notary_auth=(--key "${APPLE_NOTARY_KEY_PATH}" --key-id "${APPLE_NOTARY_KEY_ID}" --issuer "${APPLE_NOTARY_ISSUER_ID}")
  fi
fi

if (( ${#problems[@]} )); then
  printf 'release prerequisite: %s\n' "${problems[@]}" >&2
  exit 1
fi

if [[ "${mode}" == check ]]; then
  echo "local release prerequisites are configured for ${version} at ${source_sha} (notarization access is checked during packaging)"
  exit 0
fi

build_id="freqs-v${version}-${source_sha:0:12}"
release_dir="${repo_root}/dist/releases/${build_id}"
[[ ! -e "${release_dir}" ]] || { echo "release already exists: ${release_dir}" >&2; exit 1; }
mkdir -p "${repo_root}/target" "${repo_root}/dist/releases"
tmp_root="$(mktemp -d "${repo_root}/target/freqs-release.XXXXXX")"
trap 'rm -rf -- "${tmp_root}"' EXIT

libmts_pkg="${tmp_root}/ODDSound-libMTS-1.03.pkg"
# MTS-ESP clients load this shared library from a fixed system path. Ship the
# upstream signed installer alongside Freqs so one download contains both.
curl --fail --location --silent --show-error \
  "https://raw.githubusercontent.com/ODDSound/MTS-ESP/f214739b8832e7f297cb9970d0c0efbf783f1462/libMTS/Mac/x86_64_ARM/libMTSMac_v1.03.pkg" \
  --output "${libmts_pkg}"
echo "5caff5a1f194cddfe5a065cd1b2335fae76ea9d8ff21021071f75b4e7ef0fb85  ${libmts_pkg}" | shasum -a 256 -c - >/dev/null
pkg_signature="$(pkgutil --check-signature "${libmts_pkg}")"
[[ "${pkg_signature}" == *"Developer ID Installer: David Gamble (ZTMS6M322L)"* ]] || {
  echo "ODDSound libMTS installer signature is missing" >&2
  exit 1
}

bash scripts/ci.sh --vst3
VST3_SDK_DIR="${VST3_SDK_DIR}" CARGO_TARGET_DIR="${tmp_root}/cargo-target" \
  cargo rustc --release --features vst3 --lib -- -C link-arg=-Wl,-bundle
binary="${tmp_root}/cargo-target/release/libfreqs.dylib"
[[ -f "${binary}" && "$(lipo -archs "${binary}")" == arm64 ]] || {
  echo "release requires an arm64 VST3 binary" >&2
  exit 1
}
for symbol in _GetPluginFactory _bundleEntry _bundleExit; do
  /usr/bin/nm -gU "${binary}" | grep -Fq "${symbol}" || {
    echo "missing VST3 entrypoint: ${symbol}" >&2
    exit 1
  }
done
[[ "$(git rev-parse --verify HEAD)" == "${source_sha}" && -z "$(git status --porcelain --untracked-files=all)" ]] || {
  echo "release source changed during the build" >&2
  exit 1
}

bundle="${tmp_root}/freqs.vst3"
mkdir -p "${bundle}/Contents/MacOS"
cp "${binary}" "${bundle}/Contents/MacOS/freqs"
chmod 755 "${bundle}/Contents/MacOS/freqs"
cat > "${bundle}/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleDisplayName</key><string>Freqs</string>
<key>CFBundleExecutable</key><string>freqs</string>
<key>CFBundleIdentifier</key><string>com.portalsurfer.freqs.vst3</string>
<key>CFBundleName</key><string>Freqs</string>
<key>CFBundlePackageType</key><string>BNDL</string>
<key>CFBundleShortVersionString</key><string>${version}</string>
<key>CFBundleVersion</key><string>${version}</string>
<key>CFBundleSupportedPlatforms</key><array><string>MacOSX</string></array>
<key>NSHighResolutionCapable</key><true/>
</dict></plist>
EOF
printf 'BNDL????' > "${bundle}/Contents/PkgInfo"
/usr/bin/plutil -lint "${bundle}/Contents/Info.plist" >/dev/null
codesign --force --deep --timestamp --options runtime --sign "${identity}" "${bundle}"
codesign --verify --deep --strict "${bundle}"
sign_info="$(codesign -dv --verbose=4 "${bundle}" 2>&1)"
[[ "${sign_info}" == *"Authority=Developer ID Application:"* && "${sign_info}" == *"(runtime)"* ]] || {
  echo "Developer ID signature or hardened runtime is missing" >&2
  exit 1
}
[[ "${sign_info}" == *"Timestamp="* && "${sign_info}" != *"Timestamp=none"* ]] || {
  echo "Apple signing timestamp is missing" >&2
  exit 1
}
team_id="$(printf '%s\n' "${sign_info}" | sed -n 's/^TeamIdentifier=//p')"
[[ "${team_id}" =~ ^[A-Z0-9]{10}$ ]] || { echo "Developer ID TeamIdentifier is missing" >&2; exit 1; }

/usr/bin/ditto -c -k --sequesterRsrc --keepParent "${bundle}" "${tmp_root}/notary.zip"
xcrun notarytool submit "${tmp_root}/notary.zip" "${notary_auth[@]}" --wait --output-format json > "${tmp_root}/notary.json"
read -r notary_status notary_id < <(python3 - "${tmp_root}/notary.json" <<'PY'
import json, sys
result = json.load(open(sys.argv[1], encoding="utf-8"))
print(result.get("status", ""), result.get("id", ""))
PY
)
[[ "${notary_status}" == Accepted && -n "${notary_id}" ]] || {
  echo "notarization was not accepted (status: ${notary_status:-unknown}, id: ${notary_id:-none})" >&2
  exit 1
}
xcrun stapler staple "${bundle}"
xcrun stapler validate "${bundle}"
codesign -vvvv -R=notarized --check-notarization "${bundle}" >/dev/null

staged="${tmp_root}/${build_id}"
mkdir -p "${staged}"
cp "${libmts_pkg}" "${staged}/ODDSound-libMTS-1.03.pkg"
archive="${staged}/freqs-v${version}-macos.vst3.zip"
/usr/bin/ditto -c -k --sequesterRsrc --keepParent "${bundle}" "${archive}"
/usr/bin/ditto -x -k "${archive}" "${tmp_root}/audit"
audited="${tmp_root}/audit/freqs.vst3"
[[ -x "${audited}/Contents/MacOS/freqs" ]] || { echo "release ZIP is missing its VST3 binary" >&2; exit 1; }
[[ "$(/usr/bin/plutil -extract CFBundleIdentifier raw -o - "${audited}/Contents/Info.plist")" == com.portalsurfer.freqs.vst3 ]] || {
  echo "release ZIP bundle identifier changed" >&2
  exit 1
}
codesign --verify --deep --strict "${audited}"
xcrun stapler validate "${audited}"
codesign -vvvv -R=notarized --check-notarization "${audited}" >/dev/null
cp CHANGELOG.md "${staged}/CHANGELOG.md"
cat > "${staged}/INSTALL.txt" <<'EOF'
Freqs is an Apple Silicon macOS MTS-ESP tuning master. You do not need the ODDSound Master plug-in.

1. If the MTS-ESP system library is not already installed, run the included
   ODDSound-libMTS-1.03.pkg. macOS will ask for an administrator password.
   This is ODDSound's signed installer for the shared library used by MTS-ESP
   masters and clients. Do not remove an existing master plug-in until you have
   checked that your client instruments connect to Freqs.
2. Install the Freqs VST3 bundle in ~/Library/Audio/Plug-Ins/VST3/ and rescan
   plug-ins in your DAW.
3. Disable any other MTS-ESP master, load Freqs and a compatible client
   instrument, then check Freqs' client count and audition the tuning.
EOF
python3 - "${staged}" "${version}" "${source_sha}" "${team_id}" "${notary_id}" <<'PY'
import hashlib, json, pathlib, sys
root = pathlib.Path(sys.argv[1])
version, source_sha, team_id, notary_id = sys.argv[2:]
archive = root / f"freqs-v{version}-macos.vst3.zip"
manifest = {
    "product": "freqs", "version": version, "source_sha": source_sha,
    "platform": "macos", "architecture": "arm64", "format": "vst3",
    "signing": "Developer ID Application", "team_id": team_id,
    "notarized": True, "stapled": True, "notary_id": notary_id,
    "archive": archive.name, "sha256": hashlib.sha256(archive.read_bytes()).hexdigest(),
    "mts_library_installer": "ODDSound-libMTS-1.03.pkg",
    "mts_library_installer_sha256": hashlib.sha256((root / "ODDSound-libMTS-1.03.pkg").read_bytes()).hexdigest(),
}
(root / "release-info.json").write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8")
PY
complete_archive="${staged}/freqs-v${version}-macos-complete.zip"
mkdir -p "${tmp_root}/complete"
cp -R "${bundle}" "${tmp_root}/complete/freqs.vst3"
cp "${libmts_pkg}" "${staged}/INSTALL.txt" "${staged}/CHANGELOG.md" "${tmp_root}/complete/"
/usr/bin/ditto -c -k --sequesterRsrc "${tmp_root}/complete" "${complete_archive}"
/usr/bin/ditto -x -k "${complete_archive}" "${tmp_root}/complete-audit"
[[ -f "${tmp_root}/complete-audit/ODDSound-libMTS-1.03.pkg" && -d "${tmp_root}/complete-audit/freqs.vst3" ]] || {
  echo "complete ZIP is missing Freqs or the MTS-ESP library installer" >&2
  exit 1
}
codesign --verify --deep --strict "${tmp_root}/complete-audit/freqs.vst3"
xcrun stapler validate "${tmp_root}/complete-audit/freqs.vst3"
echo "5caff5a1f194cddfe5a065cd1b2335fae76ea9d8ff21021071f75b4e7ef0fb85  ${tmp_root}/complete-audit/ODDSound-libMTS-1.03.pkg" | shasum -a 256 -c - >/dev/null
python3 - "${staged}/release-info.json" "${complete_archive}" <<'PY'
import hashlib, json, pathlib, sys
manifest_path, archive = map(pathlib.Path, sys.argv[1:])
manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
manifest["complete_archive"] = archive.name
manifest["complete_archive_sha256"] = hashlib.sha256(archive.read_bytes()).hexdigest()
manifest_path.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8")
PY
mv "${staged}" "${release_dir}"
echo "verified distributable release: ${release_dir}"
