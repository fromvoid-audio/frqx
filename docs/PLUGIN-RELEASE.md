# Shared local plugin-release toolkit

frqx has an opt-in `.plugin-release.toml` and pinned `release/toolkit-revision`
for the private maintainer toolkit [PORTALSURFER/plugin-release](https://github.com/PORTALSURFER/plugin-release).
Its existing `scripts/release.sh` stays available.

```sh
./scripts/plugin-release status
./scripts/plugin-release dev
```

frqx currently supports macOS only. The launcher rejects other hosts; native
Apple Silicon and Intel Mac builds use the matching architecture.
Dev builds include the current working-tree changes and are ad-hoc signed.
The build label appears in the editor and VST3 factory metadata. The inspector
checks the exact bundle identity, dev label, and VST3 entry points.

The shared tool generates source/hash/build-number development names and handles
manual patch preparation, strict version commits, and Developer ID/notarized Mac
packages. It never installs itself.

frqx is donationware, so this config has no license gate. Gumroad publishing
is connected to the existing FREQS product. After committing the intended source,
run `prepare`, inspect the version bump, then `commit`, `deploy`, and `publish`.
The `publish` command uploads the verified archive and adds its delivery-page
file embed. Superseded delivery embeds, public listing copy, and screenshots are
updated separately.
Local checks run before dev builds and release packaging; identities/profile can be overridden with
`APPLE_CODESIGN_IDENTITY` and `APPLE_NOTARY_KEYCHAIN_PROFILE`.

Mac release packaging retains the upstream signed MTS-ESP installer and CHANGELOG.
The preparation hook pins its download and verifies its SHA-256 and signature.

The shared `commit` command commits the validated version bump and pushes it to
the configured upstream. It never force-pushes; a failed push can be retried by
rerunning `commit` without preparing another version.
