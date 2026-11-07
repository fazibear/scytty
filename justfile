set positional-arguments

# Build both binaries so the GUI can start its matching local server.
build:
    cargo build --release -p scytty -p scytty-server

# Example: just run --ssh 192.168.1.2 work
run *args: build
    ./target/release/scytty "$@"

build-server:
    cargo build --release -p scytty-server

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
