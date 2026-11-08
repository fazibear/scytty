#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

bundle="$PWD/target/server-bundle"
mkdir -p "$bundle"
cargo build --release -p scytty-server
host=$(rustc -vV | awk '/^host:/ {print $2}')
targets=(x86_64-unknown-linux-musl aarch64-unknown-linux-musl)
if [[ "$(uname -s)" == Darwin ]]; then
    targets+=(aarch64-apple-darwin x86_64-apple-darwin)
fi
linker="$(rustc --print sysroot)/lib/rustlib/$host/bin/rust-lld"
for target in "${targets[@]}"; do
    rustup target add "$target"
    case "$target" in
        x86_64-unknown-linux-musl) platform=linux-x86_64 ;;
        aarch64-unknown-linux-musl) platform=linux-arm64 ;;
        aarch64-apple-darwin) platform=macos-arm64 ;;
        x86_64-apple-darwin) platform=macos-x86_64 ;;
    esac
    if [[ "$target" == "$host" ]]; then
        binary=target/release/scytty-server
    else
        if [[ "$target" == *linux-musl ]]; then
            # Rust ships musl's libc/startup objects; its LLD links either architecture.
            RUSTFLAGS="${RUSTFLAGS:-} -C linker=$linker -C linker-flavor=ld.lld" \
                cargo build --release -p scytty-server --target "$target"
        else
            cargo build --release -p scytty-server --target "$target"
        fi
        binary="target/$target/release/scytty-server"
    fi
    cp "$binary" "$bundle/scytty-server-$platform"
    if [[ "$target" == *apple-darwin ]]; then
        codesign --force --sign - "$bundle/scytty-server-$platform"
    fi
done
