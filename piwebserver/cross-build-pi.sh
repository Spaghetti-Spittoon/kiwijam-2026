#!/usr/bin/env bash
# Cross-compile piwebserver for the Raspberry Pi 3B+.
#
# The Pi runs Raspberry Pi OS 11 (bullseye, glibc 2.31), which is OLDER than this
# WSL host (Debian bookworm, glibc 2.36). A plain gcc cross-build links against
# glibc 2.3x symbols the Pi lacks and dies at startup with
# "GLIBC_2.3x not found". So we build with cargo-zigbuild and PIN the target
# glibc to 2.31 (the `.2.31` suffix on the target triple).
#
# Run inside the debian-bookworm WSL distro:
#   wsl -d debian-bookworm -- bash -lc 'bash /mnt/c/app/0_rust/kiwijam-2026/piwebserver/cross-build-pi.sh'
#
# One-time WSL setup this assumes:
#   apt-get install build-essential pkg-config gcc-aarch64-linux-gnu xz-utils
#   dpkg --add-architecture arm64 && apt-get update && apt-get install libudev-dev:arm64
#   rustup target add aarch64-unknown-linux-gnu
#   zig 0.13 extracted to ~/zig ; cargo install cargo-zigbuild
set -euo pipefail

# cargo, then zig (cargo-zigbuild invokes `zig cc` as the cross linker).
export PATH="$HOME/.cargo/bin:$HOME/zig:$PATH"
# Let pkg-config resolve the arm64 libudev (gilrs) from Debian multiarch.
export PKG_CONFIG_ALLOW_CROSS=1
export PKG_CONFIG_PATH=/usr/lib/aarch64-linux-gnu/pkgconfig
# Native ext4 target dir (fast; avoids clashing with the Windows host build).
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
