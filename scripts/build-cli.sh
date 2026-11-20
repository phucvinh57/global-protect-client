#!/bin/sh
# Keep the standalone binary out of the desktop application's output directory.
set -eu
project_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
exec cargo build \
  --manifest-path "$project_root/src-tauri/Cargo.toml" \
  --target-dir "$project_root/src-tauri/target/cli" \
  -p gp-cli -p gp-helper "$@"
