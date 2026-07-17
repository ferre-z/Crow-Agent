#!/usr/bin/env bash
# Build the crow sidecar binary and copy it to the Tauri externalBin path.
# Usage: bash scripts/build-sidecar.sh
# Run from the repo root (or any cwd — it auto-detects the root).

set -euo pipefail

ROOT=$(git rev-parse --show-toplevel 2>/dev/null || dirname "$(cd "$(dirname "$0")" && pwd)")
cd "$ROOT"

echo "→ Building crow (release)…"
cargo build --release

TARGET=$(rustc -vV | awk '/host/ {print $2}')
SIDECAR_DIR="apps/desktop/src-tauri/binaries"
mkdir -p "$SIDECAR_DIR"

if [ ! -f "target/release/crow" ]; then
  echo "ERROR: target/release/crow not found — build may have failed" >&2
  exit 1
fi

cp "target/release/crow" "$SIDECAR_DIR/crow-${TARGET}"
echo "→ Copied crow-${TARGET} to ${SIDECAR_DIR}/"
