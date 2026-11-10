set positional-arguments

# Build local + bundled remote servers, then embed them in the GUI.
build:
    #!/usr/bin/env bash
    set -euo pipefail
    bash scripts/build-servers.sh
    SCYTTY_SERVER_BUNDLE="$PWD/target/server-bundle" cargo build --release -p scytty
    if [[ "$(uname -s)" == Darwin ]]; then
        app=target/release/Scytty.app
        mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
        cp target/release/scytty target/release/scytty-server "$app/Contents/MacOS/"
        cp assets/scytty.icns "$app/Contents/Resources/"
        version=$(cargo metadata --no-deps --format-version 1 | python3 -c 'import json,sys; print(next(p["version"] for p in json.load(sys.stdin)["packages"] if p["name"] == "scytty"))')
        python3 -c 'import plistlib,sys; plistlib.dump(dict(CFBundleName="Scytty", CFBundleDisplayName="Scytty", CFBundleIdentifier="app.scytty.terminal", CFBundleExecutable="scytty", CFBundleIconFile="scytty.icns", CFBundlePackageType="APPL", CFBundleShortVersionString=sys.argv[2], CFBundleVersion=sys.argv[2], NSHighResolutionCapable=True), open(sys.argv[1], "wb"))' "$app/Contents/Info.plist" "$version"
        plutil -lint "$app/Contents/Info.plist"
        codesign --force --sign - "$app/Contents/MacOS/scytty-server"
        codesign --force --sign - "$app"
    fi

# Build and install the macOS app.
install: build
    #!/usr/bin/env bash
    set -euo pipefail
    if [[ "$(uname -s)" != Darwin ]]; then
        echo 'just install requires macOS' >&2
        exit 1
    fi
    mkdir -p "$HOME/Applications"
    ditto target/release/Scytty.app "$HOME/Applications/Scytty.app"

# Example: just run --ssh 192.168.1.2 work
run *args: build
    #!/usr/bin/env bash
    set -euo pipefail
    if [[ "$(uname -s)" == Darwin ]]; then
        open -n target/release/Scytty.app --args "$@"
    else
        exec ./target/release/scytty "$@"
    fi

build-server:
    cargo build --release -p scytty-server

# Linux x86_64/ARM64; also macOS ARM64/x86_64 when building on macOS.
build-servers:
    bash scripts/build-servers.sh

# Requires Zig and: rustup target add x86_64-unknown-linux-gnu
build-linux:
    #!/usr/bin/env bash
    set -euo pipefail
    linker_dir=$(mktemp -d)
    trap 'rm -rf "$linker_dir"' EXIT
    printf '%s\n' '#!/bin/sh' 'exec zig cc -target x86_64-linux-gnu.2.28 "$@"' > "$linker_dir/linker"
    chmod +x "$linker_dir/linker"
    CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER="$linker_dir/linker" \
        cargo build --release -p scytty-server --target x86_64-unknown-linux-gnu

# Replace the binary atomically; leave the running daemon and its sessions alone.
install-linux: build-linux
    ssh 192.168.1.2 'mkdir -p ~/.local/bin'
    scp target/x86_64-unknown-linux-gnu/release/scytty-server 192.168.1.2:.local/bin/scytty-server.new
    ssh 192.168.1.2 'chmod 755 ~/.local/bin/scytty-server.new && mv -f ~/.local/bin/scytty-server.new ~/.local/bin/scytty-server'
