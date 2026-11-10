#!/usr/bin/env bash
set -euo pipefail

if [[ $# != 3 ]]; then
    echo "Usage: bash scripts/package-macos.sh <gui> <server> <output.app>" >&2
    exit 1
fi
cd "$(dirname "$0")/.."
gui=$1
server=$2
app=$3
test -f "$gui"
test -f "$server"
[[ "$app" == *.app ]]
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$gui" "$app/Contents/MacOS/scytty"
cp "$server" "$app/Contents/MacOS/scytty-server"
cp assets/scytty.icns "$app/Contents/Resources/"
chmod 755 "$app/Contents/MacOS/scytty" "$app/Contents/MacOS/scytty-server"
version=$(cargo metadata --no-deps --format-version 1 | python3 -c 'import json,sys; print(next(p["version"] for p in json.load(sys.stdin)["packages"] if p["name"] == "scytty"))')
python3 - "$app/Contents/Info.plist" "$version" <<'PY'
import plistlib
import sys
with open(sys.argv[1], "wb") as output:
    plistlib.dump({
        "CFBundleName": "Scytty",
        "CFBundleDisplayName": "Scytty",
        "CFBundleIdentifier": "app.scytty.terminal",
        "CFBundleExecutable": "scytty",
        "CFBundleIconFile": "scytty.icns",
        "CFBundlePackageType": "APPL",
        "CFBundleShortVersionString": sys.argv[2],
        "CFBundleVersion": sys.argv[2],
        "NSHighResolutionCapable": True,
    }, output)
PY
plutil -lint "$app/Contents/Info.plist"
test -x "$app/Contents/MacOS/scytty"
test -x "$app/Contents/MacOS/scytty-server"
codesign --force --sign - "$app/Contents/MacOS/scytty-server"
codesign --force --sign - "$app"
codesign --verify --deep --strict "$app"
