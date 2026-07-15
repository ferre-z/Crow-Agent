#!/usr/bin/env bash
# install.sh — one-line installer for Crow.
#
# Usage:
#   curl -sSf https://raw.githubusercontent.com/ferre-z/Crow-Agent/main/scripts/install.sh | sh
#
# Modes:
#   default           : clone -> cargo install -> verify -> print PATH hint
#   --test            : clone -> run `make test` (no install)
#   --path <DIR>      : use DIR as the working directory (default $TMPDIR/crow-install-$$)
#   --branch <REF>    : git ref to clone (default: main)
#   --repo <URL>      : git URL (default: https://github.com/ferre-z/Crow-Agent.git)
#   --no-path-hint    : skip the PATH reminder
#
# Requires: bash, git, cargo (rustup). Linux + macOS only; Windows is
# blocked upstream by the `nix` crate.

set -euo pipefail

REPO_DEFAULT="https://github.com/ferre-z/Crow-Agent.git"
BRANCH_DEFAULT="main"

repo="$REPO_DEFAULT"
branch="$BRANCH_DEFAULT"
mode="install"
workdir=""
print_path_hint=1
bin_name="crow"

usage() {
  cat <<'EOF'
install.sh — Crow installer

Usage:
  curl -sSf https://raw.githubusercontent.com/ferre-z/Crow-Agent/main/scripts/install.sh | sh
  ... | sh -s -- [options]

Options:
  --test           run `make test` instead of installing
  --path DIR       working directory (default $TMPDIR/crow-install-$$)
  --branch REF     git ref (default: main)
  --repo URL       git URL (default: ferre-z/Crow-Agent)
  --no-path-hint   don't print the PATH export reminder
  -h, --help       show this help

Examples:
  # Install (default):
  curl -sSf .../install.sh | sh

  # Clone + run tests without installing:
  curl -sSf .../install.sh | sh -s -- --test

  # Install a specific branch into a custom dir:
  curl -sSf .../install.sh | sh -s -- --branch my-feature --path /tmp/crow-dev
EOF
}

# Parse flags.
while [ $# -gt 0 ]; do
  case "$1" in
    --test)           mode="test"; shift ;;
    --path)           workdir="${2:-}"; shift 2 || { echo "install.sh: --path requires an argument" >&2; exit 1; } ;;
    --path=*)         workdir="${1#*=}"; shift ;;
    --branch)         branch="${2:-}"; shift 2 || { echo "install.sh: --branch requires an argument" >&2; exit 1; } ;;
    --branch=*)       branch="${1#*=}"; shift ;;
    --repo)           repo="${2:-}"; shift 2 || { echo "install.sh: --repo requires an argument" >&2; exit 1; } ;;
    --repo=*)         repo="${1#*=}"; shift ;;
    --no-path-hint)   print_path_hint=0; shift ;;
    -h|--help)        usage; exit 0 ;;
    *)                echo "install.sh: unknown argument: $1" >&2; usage >&2; exit 1 ;;
  esac
done

# --- platform check ----------------------------------------------------------
uname_s="$(uname -s)"
case "$uname_s" in
  Linux|Darwin) ;;
  *) echo "install.sh: unsupported platform '$uname_s' (Linux and macOS only)" >&2; exit 1 ;;
esac

# --- tool check --------------------------------------------------------------
for tool in git cargo make; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "install.sh: missing required tool: $tool" >&2
    case "$tool" in
      cargo|make)
        echo "  install rustup from https://rustup.rs and re-run this script." >&2
        ;;
      git)
        echo "  install git (e.g. 'apt install git' on Debian/Ubuntu, 'brew install git' on macOS)." >&2
        ;;
    esac
    exit 1
  fi
done

# --- working directory -------------------------------------------------------
if [ -z "$workdir" ]; then
  : "${TMPDIR:=/tmp}"
  workdir="$TMPDIR/crow-install-$$"
fi

# Resolve cargo home so install target is correct.
: "${CARGO_HOME:=$HOME/.cargo}"
install_bin="$CARGO_HOME/bin/$bin_name"

echo "install.sh: target directory: $workdir"

# Reuse existing checkout (idempotent) when present.
if [ ! -d "$workdir/.git" ]; then
  echo "install.sh: cloning $repo (branch $branch) — depth 1"
  git clone --depth 1 --branch "$branch" "$repo" "$workdir"
else
  echo "install.sh: reusing existing checkout at $workdir"
  # Fast-forward to the latest of the requested ref without clobbering local edits.
  (cd "$workdir" && git fetch --depth 1 origin "$branch" && git checkout "$branch" && git reset --hard "origin/$branch") \
    || echo "install.sh: warning: could not fast-forward existing checkout; continuing with current tree"
fi

cd "$workdir"

# --- run --------------------------------------------------------------------
case "$mode" in
  test)
    echo "install.sh: running test suite"
    make test
    echo "install.sh: tests passed"
    ;;
  install)
    echo "install.sh: building and installing to $install_bin"
    # `make install` wraps `cargo install --path . --locked`.
    make install
    echo ""
    echo "install.sh: installed."
    if [ -x "$install_bin" ]; then
      echo "  path: $install_bin"
      echo "  version: $($install_bin --version)"
    else
      echo "install.sh: WARNING: $install_bin not found after install" >&2
      exit 1
    fi
    if [ "$print_path_hint" = "1" ]; then
      case ":$PATH:" in
        *":$CARGO_HOME/bin:"*) : ;;  # already on PATH
        *)
          echo ""
          echo "  ~/.cargo/bin is not on PATH. Add it:"
          echo "    export PATH=\"$CARGO_HOME/bin:\$PATH\""
          ;;
      esac
    fi
    echo ""
    echo "Try:"
    echo "  $bin_name --version"
    echo "  $bin_name doctor"
    if [ -z "${NVIDIA_API_KEY:-}" ] && [ -z "${CROW_API_KEY:-}" ]; then
      echo "  (doctor will warn 'no API key configured' — that's the mock-provider fallback.)"
    fi
    ;;
esac
