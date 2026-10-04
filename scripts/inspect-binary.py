#!/usr/bin/env python3
"""Validate the exact macOS VST3 binary produced by the shared release toolkit."""
from pathlib import Path
import plistlib
import subprocess
import sys

if sys.platform != "darwin":
    raise SystemExit("Freqs currently supports macOS only")
binary = Path(sys.argv[1]).resolve()
bundle = binary.parent.parent.parent
if bundle.suffix != ".vst3" or binary.parent.name != "MacOS":
    raise SystemExit("Expected a macOS VST3 bundle binary")
with (bundle / "Contents/Info.plist").open("rb") as source:
    info = plistlib.load(source)
if info.get("CFBundleIdentifier") != "com.portalsurfer.freqs.vst3":
    raise SystemExit("Wrong Freqs bundle identifier")
if info.get("CFBundleExecutable") != binary.name:
    raise SystemExit("Bundle executable does not match packaged binary")
exports = subprocess.check_output(["nm", "-gU", str(binary)], text=True)
symbols = {line.split()[-1] for line in exports.splitlines() if line.split()}
if not {"_GetPluginFactory", "_bundleEntry", "_bundleExit"} <= symbols:
    raise SystemExit("Packaged binary is missing VST3 entry points")
if bundle.name.startswith("freqs-"):
    label = bundle.stem.removeprefix("freqs-")
    if label.encode() not in binary.read_bytes():
        raise SystemExit("Packaged binary is missing its dev build label")
print(f"Validated macOS VST3 identity and entry points: {bundle.name}")
