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
for tool in cargo codesign security xcrun ditto lipo python3; do
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
Install the Freqs VST3 bundle in ~/Library/Audio/Plug-Ins/VST3/ and rescan plug-ins in your DAW.
Freqs is an Apple Silicon macOS VST3 instrument and MTS-ESP master.
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
}
(root / "release-info.json").write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8")
PY
mv "${staged}" "${release_dir}"
echo "verified distributable release: ${release_dir}"
