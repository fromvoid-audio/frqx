# Shared local plugin-release toolkit

Freqs has an opt-in `.plugin-release.toml` and pinned `release/toolkit-revision`
for [PORTALSURFER/plugin-release](https://github.com/PORTALSURFER/plugin-release).
Its existing `scripts/release.sh` stays available.

```sh
./scripts/plugin-release status
./scripts/plugin-release dev
```

Use `scripts/plugin-release.ps1` on Windows if the plugin supports that host.
The shared tool generates source/hash/build-number development names and handles
manual patch preparation, strict version commits, Developer ID/notarized Mac
packages and unsigned Windows VST3 folder packages. It never installs itself.

Freqs is donationware, so this config has no license gate. Gumroad publishing
is intentionally unconfigured: add the correct `gumroad_product`, optional
thumbnail entries before running `publish`.
Checks run before packaging; identities/profile can be overridden with
`APPLE_CODESIGN_IDENTITY` and `APPLE_NOTARY_KEYCHAIN_PROFILE`.

Mac release packaging retains the upstream signed MTS-ESP installer and CHANGELOG.
The preparation hook pins its download and verifies its SHA-256 and signature.
