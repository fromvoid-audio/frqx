Development
===========

The repository is independent inside the AudioDev meta-workspace.

Local checks:

```fish
set -gx VST3_SDK_DIR /path/to/vst3sdk
bash scripts/ci.sh
bash scripts/ci.sh --vst3
```

For local-only Toybox iteration, use an untracked .cargo/config.toml path patch. Do not commit path patches or branch pins.
