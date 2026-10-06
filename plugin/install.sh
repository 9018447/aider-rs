#!/usr/bin/env bash
# Build aider-rs and stage the binary into the plugin directory.
# The build target defaults to a local-disk directory because network/CLOUD
# mounted filesystems corrupt Cargo artifacts; override with CARGO_TARGET_DIR.
set -euo pipefail
cd "$(dirname "$0")/.."

TARGET_DIR="${CARGO_TARGET_DIR:-/tmp/aider-rs-target}"
CARGO_TARGET_DIR="$TARGET_DIR" cargo build --release
mkdir -p plugin/bin
# cp to a temp name then rename: replacing the directory entry works even
# while a live MCP server process is executing the old binary (ETXTBSY
# otherwise), and cloud mounts may strip the exec bit on cp.
cp "$TARGET_DIR/release/aider-rs" plugin/bin/.aider-rs.new
mv -f plugin/bin/.aider-rs.new plugin/bin/aider-rs
chmod +x plugin/bin/aider-rs
echo "staged: plugin/bin/aider-rs ($(du -m plugin/bin/aider-rs | cut -f1)MB)"

# Globally install the plugin so its MCP server + dispatch skill are available
# in every repo, not just when a session is opened right inside this source tree.
# This Claude Code version loads a plugin that lives under ~/.claude/skills/<name>/
# as a "skills-dir" plugin (claude plugin init scaffolds exactly there; the plugin
# auto-loads next session as <name>@skills-dir). `claude plugin install <path>` is
# marketplace-only here, so a local plugin is distributed by mirroring its root
# directory (the one containing .claude-plugin/) into the skills dir.
if command -v claude >/dev/null 2>&1; then
  SKILLS_DIR="${CLAUDE_SKILLS_DIR:-$HOME/.claude/skills}"
  PLUGIN_DEST="$SKILLS_DIR/aider-rs"
  mkdir -p "$SKILLS_DIR"
  echo "installing plugin -> skills-dir location: $PLUGIN_DEST"
  rm -rf "$PLUGIN_DEST"
  cp -R plugin "$PLUGIN_DEST"
  chmod -R +x "$PLUGIN_DEST/bin"
  echo "installed. takes effect next session; verify with: claude plugin list (look for aider-rs@skills-dir)"
else
  echo "WARN: 'claude' CLI not found on PATH; plugin staged for a normal build but NOT installed."
  echo "      run manually: cp -R $(pwd)/plugin \$HOME/.claude/skills/aider-rs"
fi
