#!/usr/bin/env bash
# Pin glibc 2.31 because the Bullseye Pi cannot load Bookworm-linked binaries.
# Run inside the debian-bookworm WSL distro:
#   wsl -d debian-bookworm -- bash -lc 'bash /mnt/c/app/0_rust/kiwijam-2026/piwebserver/cross-build-pi.sh'
#
# One-time prerequisites:
#   apt-get install build-essential pkg-config gcc-aarch64-linux-gnu xz-utils
#   dpkg --add-architecture arm64 && apt-get update && apt-get install libudev-dev:arm64
#   rustup target add aarch64-unknown-linux-gnu
#   zig 0.13 extracted to ~/zig ; cargo install cargo-zigbuild
set -euo pipefail

export PATH="$HOME/.cargo/bin:$HOME/zig:$PATH"
export PKG_CONFIG_ALLOW_CROSS=1
export PKG_CONFIG_PATH=/usr/lib/aarch64-linux-gnu/pkgconfig
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$HOME/kiwi-target}"

cd "$(dirname "$0")"
cargo zigbuild --release --target aarch64-unknown-linux-gnu.2.31

BIN="$CARGO_TARGET_DIR/aarch64-unknown-linux-gnu/release/piwebserver"
echo
echo "Built: $BIN"
aarch64-linux-gnu-readelf -h "$BIN" | grep -E 'Class|Machine'
echo "max GLIBC required: $(aarch64-linux-gnu-objdump -T "$BIN" | grep -oE 'GLIBC_[0-9.]+' | sort -V | uniq | tail -1) (Pi has 2.31)"
echo
echo "Deploy to the Pi (/opt):"
echo "  scp \"$BIN\" pi:~/piwebserver"
echo "  ssh pi 'sudo install -o root -g root -m 755 ~/piwebserver /opt/piwebserver/piwebserver && rm ~/piwebserver'"
