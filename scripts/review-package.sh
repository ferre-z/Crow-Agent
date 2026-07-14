#!/usr/bin/env bash
# Generate a review package (commit log, stat, full diff) for a range.
# Usage: scripts/review-package.sh BASE_REF HEAD_REF
set -euo pipefail

if [ $# -ne 2 ]; then
  echo "usage: $0 BASE_REF HEAD_REF" >&2
  exit 64
fi

BASE="$1"
HEAD="$2"

# Resolve to SHAs
BASE_SHA=$(git rev-parse "$BASE")
HEAD_SHA=$(git rev-parse "$HEAD")

if [ "$BASE_SHA" = "$HEAD_SHA" ]; then
  echo "error: BASE and HEAD resolve to the same commit ($BASE_SHA)" >&2
  exit 65
fi

OUT_DIR=".review-packages"
mkdir -p "$OUT_DIR"
STAMP=$(date +%Y%m%d_%H%M%S)
OUT="$OUT_DIR/review-${BASE_SHA:0:7}-${HEAD_SHA:0:7}-${STAMP}.md"

{
  echo "# Review package"
  echo
  echo "BASE: $BASE ($BASE_SHA)"
  echo
  echo "HEAD: $HEAD ($HEAD_SHA)"
  echo
  echo "## Commits"
  echo
  echo '```'
  git log --oneline "$BASE_SHA..$HEAD_SHA"
  echo '```'
  echo
  echo "## Diff stat"
  echo
  echo '```'
  git diff --stat "$BASE_SHA" "$HEAD_SHA"
  echo '```'
  echo
  echo "## Full diff"
  echo
  echo '```diff'
  git diff -U10 "$BASE_SHA" "$HEAD_SHA"
  echo '```'
} > "$OUT"

echo "$OUT"
