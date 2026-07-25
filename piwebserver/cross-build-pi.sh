#!/usr/bin/env bash
# Cross-compile piwebserver for the Raspberry Pi 3B+ (aarch64, Debian/Pi OS
# bookworm, glibc 2.36). Run inside the `debian-bookworm` WSL distro:
#
#   wsl -d debian-bookworm -- bash /mnt/c/app/0_rust/kiwijam-2026/piwebserver/cross-build-pi.sh
#
# One-time WSL setup this assumes:
#   apt-get install build-essential pkg-config gcc-aarch64-linux-gnu
#   dpkg --add-architecture arm64 && apt-get update && apt-get install libudev-dev:arm64
#   rustup target add aarch64-unknown-linux-gnu
set -euo pipefail

export PATH="$HOME/.cargo/bin:$PATH"
# aarch64 cross linker (gilrs/libudev is a C dep, so we need a real linker).
export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc
# Let pkg-config resolve the arm64 libudev from Debian multiarch.
export PKG_CONFIG_ALLOW_CROSS=1
export PKG_CONFIG_PATH=/usr/lib/aarch64-linux-gnu/pkgconfig
# Build into the native WSL filesystem (fast; avoids clashing with the Windows
# host build under target/). Override by exporting CARGO_TARGET_DIR yourself.
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$HOME/kiwi-target}"

cd "$(dirname "$0")"
cargo build --release --target aarch64-unknown-linux-gnu

BIN="$CARGO_TARGET_DIR/aarch64-unknown-linux-gnu/release/piwebserver"
echo
echo "Built: $BIN"
file "$BIN" 2>/dev/null || aarch64-linux-gnu-readelf -h "$BIN" | grep -E 'Class|Machine'
echo "Deploy, e.g.:  scp \"$BIN\" pi@raspberrypi.local:~/"
