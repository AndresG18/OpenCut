#!/bin/sh
set -eu

plugin_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)

if [ -n "${OPENCUT_REPO_ROOT:-}" ]; then
  repo_root=$OPENCUT_REPO_ROOT
elif [ -f "$plugin_root/../../Cargo.toml" ] && [ -d "$plugin_root/../../crates/opencut-mcp" ]; then
  repo_root=$(CDPATH= cd -- "$plugin_root/../.." && pwd)
elif [ -f "$PWD/Cargo.toml" ] && [ -d "$PWD/crates/opencut-mcp" ]; then
  repo_root=$PWD
else
  echo "OpenCut repository not found. Run this plugin from the OpenCut checkout or set OPENCUT_REPO_ROOT." >&2
  exit 1
fi

if [ -x "$repo_root/target/debug/opencut-mcp" ]; then
  exec "$repo_root/target/debug/opencut-mcp"
fi

exec cargo run --quiet --manifest-path "$repo_root/Cargo.toml" -p opencut-mcp
