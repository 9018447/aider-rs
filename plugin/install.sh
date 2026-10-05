#!/usr/bin/env bash
# Build aider-rs and stage the binary into the plugin directory.
# The build target defaults to a local-disk directory because network/CLOUD
# mounted filesystems corrupt Cargo artifacts; override with CARGO_TARGET_DIR.
set -euo pipefail
cd "$(dirname "$0")/.."

TARGET_DIR="${CARGO_TARGET_DIR:-/tmp/aider-rs-target}"
CARGO_TARGET_DIR="$TARGET_DIR" cargo build --release
mkdir -p plugin/bin
cp "$TARGET_DIR/release/aider-rs" plugin/bin/aider-rs
echo "staged: plugin/bin/aider-rs ($(du -m plugin/bin/aider-rs | cut -f1)MB)"
