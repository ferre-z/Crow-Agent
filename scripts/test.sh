#!/usr/bin/env bash
# test.sh — convenience wrapper that runs the offline test suite.
#
# Run from any cwd; resolves the repo root from this script's path.
# Equivalent to `make test` but useful when the user reaches for a
# shell script rather than remembering the Makefile target.
#
# Tests are deterministic: no API key, no network.

set -euo pipefail
# cd to the directory holding this script, then up one level.
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$here/.." && pwd)"
cd "$repo_root"

make test "$@"
