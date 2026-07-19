#!/usr/bin/env bash
# install.sh — one-line installer for Crow.
#
# Usage:
#   curl -sSf https://raw.githubusercontent.com/ferre-z/Crow-Agent/main/scripts/install.sh | sh
#
# Modes:
#   default           : clone -> npm install + build -> install bin
#   --test            : clone -> run tests; no install
#   --no-build        : skip the TS build (faster; uses prebuilt)
#   --path <DIR>      : use DIR as the working directory (default $TMPDIR/crow-install-$$)
#   --branch <REF>    : git ref to clone (default: main)
#   --repo <URL>      : git URL (default: https://github.com/ferre-z/Crow-Agent.git)
#   --no-bootstrap    : skip auto-install of missing tools
#   --no-path-hint    : skip the PATH export reminder
#   -y, --yes         : skip the disk-quota preflight check
#
# Auto-bootstraps:
#   - Node.js >= 18 via NodeSource / nvm / system when missing.
#   - git, curl via the system package manager.
#
# Requires: bash, uname. Linux + macOS only (Windows: use WSL).

set -euo pipefail

REPO_DEFAULT="https://github.com/ferre-z/Crow-Agent.git"
BRANCH_DEFAULT="main"

repo="$REPO_DEFAULT"
branch="$BRANCH_DEFAULT"
mode="install"
build_ts=1            # default: build the TypeScript bundle
workdir=""
print_path_hint=1
bootstrap=1
yes_preflight=0
bin_name="crow"

usage() {
  cat <<'EOF'
install.sh — Crow installer

Usage:
  curl -sSf https://raw.githubusercontent.com/ferre-z/Crow-Agent/main/scripts/install.sh | sh
  ... | sh -s -- [options]

Options:
  --test           run tests instead of installing
  --no-build       skip the TypeScript build (faster; uses prebuilt)
  --path DIR       working directory (default $TMPDIR/crow-install-$$)
  --branch REF     git ref (default: main)
  --repo URL       git URL (default: ferre-z/Crow-Agent)
  --no-bootstrap   don't auto-install missing tools
  --no-path-hint   don't print the PATH export reminder
  -h, --help       show this help

The script auto-installs Node.js (>= 18) and git/curl via the
system package manager when missing, so the one-liner works on a
clean machine.

After install, run:
  crow tui                  # interactive Claude-Code-style REPL
  crow -p "say hi"          # one-shot
  crow --mode json -p ...   # streaming JSON for CI
  crow install npm:@crow/coding-agent  # optional Crow extensions
EOF
}

# --- output formatting -------------------------------------------------------
_color=0
if [ -t 1 ] && [ -z "${NO_COLOR:-}" ]; then
  _color=1
fi
if [ "$_color" = "1" ]; then
  _green='\033[32m'; _red='\033[31m'; _yellow='\033[33m'
  _cyan='\033[36m'; _bold='\033[1m'; _reset='\033[0m'
else
  _green=''; _red=''; _yellow=''; _cyan=''; _bold=''; _reset=''
fi
phase() { printf "${_cyan}${_bold}==> %s${_reset}\n" "$1"; }
ok()    { printf "  ${_green}✓${_reset} %s\n" "$1"; }
warn()  { printf "  ${_yellow}!${_reset} %s\n" "$1" >&2; }
fail()  { printf "  ${_red}✗${_reset} %s\n" "$1" >&2; }

# Parse flags.
while [ $# -gt 0 ]; do
  case "$1" in
    --test)         mode="test"; shift ;;
    --no-build)     build_ts=0; shift ;;
    --path)         workdir="${2:-}"; shift 2 || { fail "--path requires an argument"; exit 1; } ;;
    --path=*)       workdir="${1#*=}"; shift ;;
    --branch)       branch="${2:-}"; shift 2 || { fail "--branch requires an argument"; exit 1; } ;;
    --branch=*)     branch="${1#*=}"; shift ;;
    --repo)         repo="${2:-}"; shift 2 || { fail "--repo requires an argument"; exit 1; } ;;
    --repo=*)       repo="${1#*=}"; shift ;;
    --no-bootstrap) bootstrap=0; shift ;;
    --no-path-hint) print_path_hint=0; shift ;;
    -y|--yes)       yes_preflight=1; shift ;;
    -h|--help)      usage; exit 0 ;;
    *)              fail "unknown argument: $1"; usage >&2; exit 1 ;;
  esac
done

# --- platform check ----------------------------------------------------------
uname_s="$(uname -s)"
case "$uname_s" in
  Linux|Darwin) ;;
  *) fail "unsupported platform '$uname_s' (Linux and macOS only)"; exit 1 ;;
esac

have() { command -v "$1" >/dev/null 2>&1; }

detect_sudo() {
  if [ "$(id -u)" = "0" ]; then SUDO=""
  elif have sudo && sudo -n true 2>/dev/null; then SUDO="sudo -n"
  else SUDO=""
  fi
  export SUDO
}

install_packages() {
  detect_sudo
  case "$uname_s" in
    Darwin)
      have brew || return 0
      for pkg in "$@"; do
        have "$pkg" && continue
        echo "install.sh: installing '$pkg' via Homebrew"
        brew install "$pkg" >/dev/null 2>&1 || true
      done
      ;;
    Linux)
      [ -f /etc/os-release ] || return 0
      . /etc/os-release
      case "${ID:-unknown}" in
        ubuntu|debian|pop|linuxmint|elementary|kde-neon)
          for pkg in "$@"; do
            have "$pkg" && continue
            $SUDO apt-get update -qq >/dev/null 2>&1 || true
            $SUDO apt-get install -y "$pkg" >/dev/null 2>&1 || true
          done ;;
        fedora|rhel|centos|rocky|almalinux|ol)
          for pkg in "$@"; do
            have "$pkg" && continue
            $SUDO dnf install -y "$pkg" >/dev/null 2>&1 || true
          done ;;
        arch|manjaro|endeavouros)
          for pkg in "$@"; do
            have "$pkg" && continue
            $SUDO pacman -Sy --noconfirm --needed "$pkg" >/dev/null 2>&1 || true
          done ;;
        opensuse*|sles)
          for pkg in "$@"; do
            have "$pkg" && continue
            $SUDO zypper --non-interactive install -y "$pkg" >/dev/null 2>&1 || true
          done ;;
        alpine)
          for pkg in "$@"; do
            have "$pkg" && continue
            $SUDO apk add --no-cache "$pkg" >/dev/null 2>&1 || true
          done ;;
      esac ;;
  esac
}

scan_dependencies() {
  _dep_names=(); _dep_status=(); _dep_paths=()
  _missing_system_pkgs=()
  for tool in git curl node npm pnpm; do
    _dep_names+=("$tool")
    if have "$tool"; then
      _dep_status+=("ok"); _dep_paths+=("$(command -v "$tool")")
    else
      _dep_status+=("missing"); _dep_paths+=("")
      # npm is bundled with Node; we install Node then npm is present.
      case "$tool" in
        node|npm) : ;;
        *) _missing_system_pkgs+=("$tool") ;;
      esac
    fi
  done
}

print_dep_table() {
  echo "dependency check:"
  for i in "${!_dep_names[@]}"; do
    local name="${_dep_names[$i]}" status="${_dep_status[$i]}" path="${_dep_paths[$i]}"
    if [ "$status" = "ok" ]; then
      printf "  %-8s ✓ %s\n" "$name" "$path"
    else
      printf "  %-8s ✗ missing\n" "$name"
    fi
  done
}

# Install Node via NodeSource (Linux) or Homebrew (macOS).
install_node() {
  detect_sudo
  case "$uname_s" in
    Darwin)
      if have brew; then
        brew install node@20 >/dev/null 2>&1 || true
      fi
      ;;
    Linux)
      [ -f /etc/os-release ] || return 0
      . /etc/os-release
      case "${ID:-unknown}" in
        ubuntu|debian|pop|linuxmint|elementary|kde-neon)
          if ! have curl; then return 0; fi
          echo "install.sh: installing Node.js 20.x via NodeSource"
          curl --proto '=https' --tlsv1.2 -fsSL https://deb.nodesource.com/setup_20.x \
            | $SUDO bash - >/dev/null 2>&1 || true
          $SUDO apt-get install -y nodejs >/dev/null 2>&1 || true
          ;;
        fedora|rhel|centos|rocky|almalinux|ol)
          echo "install.sh: installing Node.js 20.x via NodeSource"
          curl --proto '=https' --tlsv1.2 -fsSL https://rpm.nodesource.com/setup_20.x \
            | $SUDO bash - >/dev/null 2>&1 || true
          $SUDO dnf install -y nodejs >/dev/null 2>&1 || true
          ;;
        arch|manjaro|endeavouros)
          $SUDO pacman -Sy --noconfirm --needed nodejs npm >/dev/null 2>&1 || true
          ;;
        alpine)
          $SUDO apk add --no-cache nodejs npm >/dev/null 2>&1 || true
          ;;
      esac
      ;;
  esac
}

# --- bootstrap ---------------------------------------------------------------
if [ "$bootstrap" = "1" ]; then
  phase "Checking dependencies"
  scan_dependencies
  print_dep_table

  if [ ${#_missing_system_pkgs[@]} -gt 0 ]; then
    phase "Installing system packages"
    install_packages "${_missing_system_pkgs[@]}"
    for pkg in "${_missing_system_pkgs[@]}"; do
      have "$pkg" || { fail "'$pkg' could not be auto-installed"; exit 1; }
      ok "$pkg installed"
    done
  else
    ok "All system packages present"
  fi

  if ! have node; then
    phase "Installing Node.js"
    install_node
    have node || { fail "node still not on PATH after install"; exit 1; }
    ok "Node $(node --version) installed"
  else
    ok "Node $(node --version) present"
  fi
fi

# Final check.
missing=()
for tool in git curl node; do
  have "$tool" || missing+=("$tool")
done
if [ ${#missing[@]} -gt 0 ]; then
  fail "required tools still missing: ${missing[*]}"
  echo "  Install them manually, then re-run this script (or pass --no-bootstrap)." >&2
  exit 1
fi

# --- working directory -------------------------------------------------------
: "${TMPDIR:=/tmp}"
if [ -z "$workdir" ]; then workdir="$TMPDIR/crow-install-$$"; fi

# Install location: $CARGO_HOME/bin equivalent. We use ~/.local/bin since
# Crow is now a node CLI; ~/.cargo/bin doesn't apply. Override with
# CROW_INSTALL_BIN.
: "${CROW_INSTALL_BIN:=$HOME/.local/bin}"
install_bin="$CROW_INSTALL_BIN/$bin_name"

phase "Preparing source"
echo "  directory: $workdir"

if [ ! -d "$workdir/.git" ]; then
  phase "Cloning repository"
  git clone --depth 1 --branch "$branch" "$repo" "$workdir"
  ok "Cloned $repo (branch $branch)"
else
  phase "Updating existing checkout"
  (cd "$workdir" && git fetch --depth 1 origin "$branch" && git checkout "$branch" && git reset --hard "origin/$branch") \
    || warn "Could not fast-forward; using current tree"
  ok "Reusing $workdir"
fi

cd "$workdir"

# --- run --------------------------------------------------------------------
case "$mode" in
  test)
    phase "Running tests"
    cd pi-crow
    npm install --ignore-scripts >/dev/null 2>&1
    npm run check
    ok "Tests passed"
    ;;
  install)
    phase "Installing pi-crow workspace"
    cd pi-crow
    npm install --ignore-scripts
    ok "npm install complete"

    if [ "$build_ts" = "1" ]; then
      phase "Building TypeScript bundle"
      npm run build
      ok "TypeScript build complete"
    fi

    phase "Installing $bin_name to $CROW_INSTALL_BIN"
    mkdir -p "$CROW_INSTALL_BIN"
    cat > "$install_bin" <<EOF
#!/usr/bin/env bash
# Crow launcher — forwards all args to the local pi-crow bundle.
exec node "$(cd "$workdir" && pwd)/pi-crow/packages/coding-agent/dist/cli.js" "\$@"
EOF
    chmod +x "$install_bin"
    ok "$install_bin"

    if [ -x "$install_bin" ]; then
      echo ""
      echo "┌─────────────────────────────────────────────┐"
      echo "│  Installation complete!                     │"
      echo "├─────────────────────────────────────────────┤"
      printf "│  path:    %-34s│\n" "$install_bin"
      printf "│  version: %-34s│\n" "$($install_bin --version 2>/dev/null || echo 'unknown')"
      echo "└─────────────────────────────────────────────┘"
    fi

    if [ "$print_path_hint" = "1" ]; then
      case ":$PATH:" in
        *":$CROW_INSTALL_BIN:"*) : ;;
        *)
          echo ""
          echo "  $CROW_INSTALL_BIN is not on PATH. Add it:"
          echo "    export PATH=\"$CROW_INSTALL_BIN:\$PATH\""
          ;;
      esac
    fi

    echo ""
    echo "Try:"
    echo "  crow tui                  # interactive Claude-Code-style REPL"
    echo "  crow -p \"say hi\"          # one-shot"
    if [ -z "${NVIDIA_API_KEY:-}" ] && [ -z "${ANTHROPIC_API_KEY:-}" ] && [ -z "${OPENAI_API_KEY:-}" ]; then
      echo "  (Set NVIDIA_API_KEY (or ANTHROPIC/OPENAI_API_KEY) before chatting.)"
    fi
    ;;
esac
