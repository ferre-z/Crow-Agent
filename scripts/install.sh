#!/usr/bin/env bash
#
# crow install — single-line installer.
#
#   curl -sSf https://raw.githubusercontent.com/ferre-z/Crow-Agent/main/scripts/install.sh | sh
#
# Auto-bootstraps Node ≥ 22.19 (pi-mono requirement) and pnpm 9 when missing.
# Installs `crowd` (the daemon) and the `crow` CLI on PATH, plus the optional
# Electron desktop. Refuses to run on Windows — use WSL.

set -euo pipefail

REPO="${CROW_INSTALL_REPO:-ferre-z/Crow-Agent}"
BRANCH="${CROW_INSTALL_BRANCH:-main}"
PREFIX="${CROW_INSTALL_PREFIX:-$HOME/.local}"
NODE_VERSION="${CROW_NODE_VERSION:-22.19.0}"
PNPM_VERSION="${CROW_PNPM_VERSION:-9.15.0}"
INSTALL_DESKTOP="${CROW_INSTALL_DESKTOP:-1}"

if [[ "${OS:-$(uname -s)}" == "MINGW"* || "${OS:-$(uname -s)}" == "CYGWIN"* ]]; then
  echo "crow install: Windows is not supported. Use WSL." >&2
  exit 1
fi

log() { printf '\033[1;34m[install]\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33m[install]\033[0m %s\n' "$*" >&2; }
die() { printf '\033[1;31m[install]\033[0m %s\n' "$*" >&2; exit 1; }

# 0. Disk + memory awareness.
if command -v df >/dev/null 2>&1; then
  avail=$(df -k "$HOME" | awk 'NR==2 {print $4}')
  if [[ -n "$avail" && "$avail" -lt 1048576 ]]; then
    warn "less than 1 GiB free in $HOME — install may fail"
  fi
fi

# 1. Ensure Node 22.19+ is on PATH.
need_node=0
if ! command -v node >/dev/null 2>&1; then
  need_node=1
elif ! node -e 'process.exit(Number(process.versions.node.split(".")[0]) >= 22 && Number(process.versions.node.split(".")[1]) >= 19 ? 0 : 1)' 2>/dev/null; then
  need_node=1
fi

if [[ "$need_node" == "1" ]]; then
  log "installing Node $NODE_VERSION via nvm (if available) or fnm"
  if command -v nvm >/dev/null 2>&1; then
    # shellcheck disable=SC1090
    \. "$HOME/.nvm/nvm.sh"
    nvm install "$NODE_VERSION"
    nvm use "$NODE_VERSION"
  elif command -v fnm >/dev/null 2>&1; then
    fnm install "$NODE_VERSION" --use-on-cd
  else
    warn "nvm/fnm not found; falling back to downloading the official tarball"
    arch=$(uname -m)
    case "$arch" in
      x86_64) node_arch="x64" ;;
      aarch64|arm64) node_arch="arm64" ;;
      *) die "unsupported architecture: $arch" ;;
    esac
    tmpdir=$(mktemp -d)
    tarball="node-v${NODE_VERSION}-linux-${node_arch}.tar.xz"
    curl -fsSL "https://nodejs.org/dist/v${NODE_VERSION}/${tarball}" -o "$tmpdir/$tarball"
    tar -xJf "$tmpdir/$tarball" -C "$tmpdir"
    mkdir -p "$PREFIX"
    cp -R "$tmpdir/node-v${NODE_VERSION}/." "$PREFIX/"
    rm -rf "$tmpdir"
    export PATH="$PREFIX/bin:$PATH"
  fi
fi

# 2. Ensure pnpm 9.
if ! command -v pnpm >/dev/null 2>&1; then
  log "installing pnpm $PNPM_VERSION"
  npm install -g "pnpm@$PNPM_VERSION"
fi

# 3. Clone (or update) the repo.
work="$HOME/.local/share/crow"
if [[ -d "$work" ]]; then
  log "updating existing checkout at $work"
  git -C "$work" fetch --quiet
  git -C "$work" checkout --quiet "$BRANCH"
  git -C "$work" pull --quiet --rebase --autostash origin "$BRANCH"
else
  log "cloning $REPO@$BRANCH into $work"
  mkdir -p "$(dirname "$work")"
  git clone --quiet --branch "$BRANCH" "https://github.com/$REPO.git" "$work"
fi

# 4. Install workspace deps.
log "pnpm install (with build scripts allowed for electron + sqlite)"
cd "$work"
pnpm install --reporter=append-only

# 5. Build the desktop (optional — large, can be skipped via CROW_INSTALL_DESKTOP=0).
if [[ "$INSTALL_DESKTOP" == "1" ]]; then
  log "building the desktop (electron-vite)"
  pnpm --filter @crow/desktop build
fi

# 6. Symlink the binaries on PATH.
mkdir -p "$PREFIX/bin"
ln -sf "$work/packages/daemon/src/bin.ts" "$PREFIX/bin/crowd"
ln -sf "$work/apps/cli/src/bin.ts" "$PREFIX/bin/crow"
chmod +x "$work/packages/daemon/src/bin.ts" "$work/apps/cli/src/bin.ts"

# 7. Done.
log "installed."
echo
echo "  crowd:  $(command -v crowd 2>/dev/null || echo "$PREFIX/bin/crowd")"
echo "  crow:   $(command -v crow 2>/dev/null || echo "$PREFIX/bin/crow")"
echo
echo "Add $PREFIX/bin to your PATH if it isn't already:"
echo "  export PATH=\"$PREFIX/bin:\$PATH\""
echo
echo "Quick start:"
echo "  crowd            # starts the daemon on ws://127.0.0.1:7749"
echo "  crow prompt 'list the files here' --host local"
echo
echo "If \$PREFIX/bin isn't on PATH yet, run the export line above first."
