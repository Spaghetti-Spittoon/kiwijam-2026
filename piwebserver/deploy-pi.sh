#!/usr/bin/env bash
# One-shot: cross-build in WSL, copy to the Pi, install to /opt, restart the
# systemd service. Run from Windows **Git Bash** (it holds the ssh key/config;
# the Pi is not reachable from inside WSL).
#
#   ./deploy-pi.sh
#
# Prereqs: the debian-bookworm WSL distro set up per cross-build-pi.sh, and the
# `pi` host alias in ~/.ssh/config. The systemd unit (piwebserver.service) must
# already be installed on the Pi (see README / this folder).
set -euo pipefail

REPO_WSL=/mnt/c/app/0_rust/kiwijam-2026/piwebserver
OUT=/root/kiwi-target/aarch64-unknown-linux-gnu/release/piwebserver
STAGE="$REPO_WSL/piwebserver-aarch64"   # WSL path == Windows repo path

echo ">> Building (WSL debian-bookworm, zigbuild, glibc 2.31)..."
wsl -d debian-bookworm -- bash -lc "export PATH=\"\$HOME/.cargo/bin:\$HOME/zig:\$PATH\"; export PKG_CONFIG_ALLOW_CROSS=1; export PKG_CONFIG_PATH=/usr/lib/aarch64-linux-gnu/pkgconfig; export CARGO_TARGET_DIR=\"\$HOME/kiwi-target\"; cd $REPO_WSL && cargo zigbuild --release --target aarch64-unknown-linux-gnu.2.31 && cp $OUT $STAGE"

echo ">> Copying to the Pi..."
scp "$(dirname "$0")/piwebserver-aarch64" pi:~/piwebserver

echo ">> Installing to /opt and restarting the service..."
ssh pi 'sudo install -o root -g root -m 755 ~/piwebserver /opt/piwebserver/piwebserver \
        && rm ~/piwebserver \
        && sudo systemctl restart piwebserver \
        && sleep 1 \
        && systemctl is-active piwebserver'

rm -f "$(dirname "$0")/piwebserver-aarch64"
echo ">> Done. Live logs:  ssh pi 'journalctl -u piwebserver -f'"
