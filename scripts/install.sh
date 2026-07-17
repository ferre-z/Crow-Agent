#!/usr/bin/env bash
# install.sh — one-line installer for Crow.
#
# Usage:
#   curl -sSf https://raw.githubusercontent.com/ferre-z/Crow-Agent/main/scripts/install.sh | sh
#
# Modes:
#   default           : clone -> debug build + copy -> cargo clean -> verify
#   --release         : release build via `cargo install --path . --locked`
#                        (larger peak disk usage)
#   --test            : clone -> run `make test` (no install)
#   --path <DIR>      : use DIR as the working directory (default $TMPDIR/crow-install-$$)
#   --branch <REF>    : git ref to clone (default: main)
#   --repo <URL>      : git URL (default: https://github.com/ferre-z/Crow-Agent.git)
#   --no-bootstrap    : skip auto-install of missing tools
#   --no-path-hint    : skip the PATH export reminder
#   --no-clean        : skip `cargo clean` after install (debug mode only)
#   -y, --yes         : skip the disk-quota preflight check
#
# Auto-bootstraps:
#   - Rust toolchain via rustup if `cargo` is missing. Non-interactive,
#     no shell rc modification (uses ~/.cargo/env for this process only).
#   - Basic build tools (git, make, curl) via the system package
#     manager when missing. Best-effort: silently skips when
#     passwordless sudo is unavailable; in that case the script
#     tells the user what to install and exits with a clear message.
#
# Requires: bash, uname. Linux + macOS only (Windows is blocked
# upstream by the `nix` crate).

set -euo pipefail

REPO_DEFAULT="https://github.com/ferre-z/Crow-Agent.git"
BRANCH_DEFAULT="main"

repo="$REPO_DEFAULT"
branch="$BRANCH_DEFAULT"
mode="install"
build_profile="debug"  # default; override with --release
workdir=""
print_path_hint=1
bootstrap=1
clean_after=1          # default for debug install
yes_preflight=0
_verbose=0
bin_name="crow"
rust_toolchain="1.88"

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
  --no-bootstrap   don't auto-install missing tools
  --no-path-hint   don't print the PATH export reminder
  --verbose         show full cargo output during build
  -h, --help       show this help

The script auto-installs Rust (via rustup) and basic build tools (via the
system package manager) when missing, so the one-liner works on a clean
machine. Pass --no-bootstrap to opt out and require pre-installed tools.

Examples:
  # Install (default):
  curl -sSf .../install.sh | sh

  # Clone + run tests without installing:
  curl -sSf .../install.sh | sh -s -- --test

  # Install a specific branch into a custom dir:
  curl -sSf .../install.sh | sh -s -- --branch my-feature --path /tmp/crow-dev

  # Skip auto-bootstrap (require pre-installed tools):
  curl -sSf .../install.sh | sh -s -- --no-bootstrap
EOF
}

# --- output formatting -------------------------------------------------------
# TTY detection: colour when stdout is a tty, plain when piped.
# Honour NO_COLOR (https://no-color.org/).
_color=0
if [ -t 1 ] && [ -z "${NO_COLOR:-}" ]; then
  _color=1
fi

if [ "$_color" = "1" ]; then
  _green='\033[32m'
  _red='\033[31m'
  _yellow='\033[33m'
  _cyan='\033[36m'
  _bold='\033[1m'
  _reset='\033[0m'
else
  _green=''
  _red=''
  _yellow=''
  _cyan=''
  _bold=''
  _reset=''
fi

# Phase header: ==> Phase Name
phase() {
  printf "${_cyan}${_bold}==> %s${_reset}\n" "$1"
}

# Success marker: ✓ message
ok() {
  printf "  ${_green}✓${_reset} %s\n" "$1"
}

# Warning marker: ! message
warn() {
  printf "  ${_yellow}!${_reset} %s\n" "$1" >&2
}

# Error marker: ✗ message (to stderr)
fail() {
  printf "  ${_red}✗${_reset} %s\n" "$1" >&2
}

# Parse flags.
while [ $# -gt 0 ]; do
  case "$1" in
    --test)           mode="test"; shift ;;
    --release)        build_profile="release"; clean_after=0; shift ;;
    --path)           workdir="${2:-}"; shift 2 || { fail "--path requires an argument"; exit 1; } ;;
    --path=*)         workdir="${1#*=}"; shift ;;
    --branch)         branch="${2:-}"; shift 2 || { fail "--branch requires an argument"; exit 1; } ;;
    --branch=*)       branch="${1#*=}"; shift ;;
    --repo)           repo="${2:-}"; shift 2 || { fail "--repo requires an argument"; exit 1; } ;;
    --repo=*)         repo="${1#*=}"; shift ;;
    --no-bootstrap)   bootstrap=0; shift ;;
    --no-path-hint)   print_path_hint=0; shift ;;
    --no-clean)       clean_after=0; shift ;;
    -y|--yes)         yes_preflight=1; shift ;;
    --verbose)        _verbose=1; shift ;;
    -h|--help)        usage; exit 0 ;;
    *)                fail "unknown argument: $1"; usage >&2; exit 1 ;;
  esac
done

# --- platform check ----------------------------------------------------------
uname_s="$(uname -s)"
case "$uname_s" in
  Linux|Darwin) ;;
  *) fail "unsupported platform '$uname_s' (Linux and macOS only)"; exit 1 ;;
esac

# --- helpers -----------------------------------------------------------------
have() { command -v "$1" >/dev/null 2>&1; }

# Run as root: empty SUDO. Passwordless sudo (sudo -n works): SUDO=sudo -n.
# Otherwise: empty (best-effort, will silently skip installs).
detect_sudo() {
  if [ "$(id -u)" = "0" ]; then
    SUDO=""
  elif have sudo && sudo -n true 2>/dev/null; then
    SUDO="sudo -n"
  else
    SUDO=""
  fi
  export SUDO
}

# Best-effort install via the system package manager. Skips silently
# when there's no passwordless sudo or an unknown distro; in that case
# the outer check at the end reports the missing tools.
install_packages() {
  detect_sudo
  case "$uname_s" in
    Darwin)
      if ! have brew; then
        return 0
      fi
      for pkg in "$@"; do
        if have "$pkg"; then continue; fi
        echo "install.sh: installing '$pkg' via Homebrew"
        brew install "$pkg" >/dev/null 2>&1 || true
      done
      ;;
    Linux)
      if [ ! -f /etc/os-release ]; then
        return 0
      fi
      # shellcheck disable=SC1091
      . /etc/os-release
      case "${ID:-unknown}" in
        ubuntu|debian|pop|linuxmint|elementary|kde-neon)
          for pkg in "$@"; do
            if have "$pkg"; then continue; fi
            echo "install.sh: installing '$pkg' via apt-get"
            $SUDO apt-get update -qq >/dev/null 2>&1 || true
            $SUDO apt-get install -y "$pkg" >/dev/null 2>&1 || true
          done
          ;;
        fedora|rhel|centos|rocky|almalinux|ol)
          for pkg in "$@"; do
            if have "$pkg"; then continue; fi
            echo "install.sh: installing '$pkg' via dnf"
            $SUDO dnf install -y "$pkg" >/dev/null 2>&1 || true
          done
          ;;
        arch|manjaro|endeavouros)
          for pkg in "$@"; do
            if have "$pkg"; then continue; fi
            echo "install.sh: installing '$pkg' via pacman"
            $SUDO pacman -Sy --noconfirm --needed "$pkg" >/dev/null 2>&1 || true
          done
          ;;
        opensuse*|sles)
          for pkg in "$@"; do
            if have "$pkg"; then continue; fi
            echo "install.sh: installing '$pkg' via zypper"
            $SUDO zypper --non-interactive install -y "$pkg" >/dev/null 2>&1 || true
          done
          ;;
        alpine)
          for pkg in "$@"; do
            if have "$pkg"; then continue; fi
            echo "install.sh: installing '$pkg' via apk"
            $SUDO apk add --no-cache "$pkg" >/dev/null 2>&1 || true
          done
          ;;
        *)
          # Unknown distro — give up rather than guess a package manager.
          ;;
      esac
      ;;
  esac
}

# Scan all required tools and print a dependency table.
# Sets global arrays: _dep_names, _dep_status, _dep_paths
# Also sets: _need_rust (0 or 1), _missing_system_pkgs (array)
scan_dependencies() {
  _dep_names=()
  _dep_status=()
  _dep_paths=()
  _need_rust=0
  _missing_system_pkgs=()

  for tool in git curl make cargo rustc; do
    _dep_names+=("$tool")
    if have "$tool"; then
      _dep_status+=("ok")
      _dep_paths+=("$(command -v "$tool")")
    else
      _dep_status+=("missing")
      _dep_paths+=("")
      if [ "$tool" = "cargo" ] || [ "$tool" = "rustc" ]; then
        _need_rust=1
      else
        _missing_system_pkgs+=("$tool")
      fi
    fi
  done
}

# Pretty-print the dependency scan table.
print_dep_table() {
  echo "dependency check:"
  for i in "${!_dep_names[@]}"; do
    local name="${_dep_names[$i]}"
    local status="${_dep_status[$i]}"
    local path="${_dep_paths[$i]}"
    if [ "$status" = "ok" ]; then
      printf "  %-8s ✓ %s\n" "$name" "$path"
    else
      local hint=""
      if [ "$name" = "cargo" ] || [ "$name" = "rustc" ]; then
        hint=" (will install Rust ${rust_toolchain} via rustup)"
      fi
      printf "  %-8s ✗ missing%s\n" "$name" "$hint"
    fi
  done
}

# Install Rust via rustup. Uses curl (which we ensured above).
install_rust() {
  # Pre-flight: rustup-init needs to extract the rustup-init binary
  # into a tmp file. If there's no space, fail with a clear message
  # rather than getting a cryptic curl: (23) later.
  if [ -d /tmp ]; then
    local tmp_avail_kib tmp_need_kib=51200  # 50 MiB
    tmp_avail_kib=$(df -k /tmp 2>/dev/null | awk 'NR==2 { print $4 }')
    if [ -n "$tmp_avail_kib" ] && [ "$tmp_avail_kib" -lt "$tmp_need_kib" ]; then
      echo "" >&2
      fail "not enough free space in /tmp for rustup-init"
      echo "  available: $((tmp_avail_kib / 1024)) MiB" >&2
      echo "  need:      ~50 MiB" >&2
      echo "" >&2
      echo "  Free up space in /tmp (e.g. 'rm -rf /tmp/tmp.*' or 'sudo tmpreaper')" >&2
      echo "  and re-run this script." >&2
      exit 1
    fi
  fi

  echo "install.sh: installing Rust toolchain via rustup (channel ${rust_toolchain})"
  local url="https://sh.rustup.rs"
  # `--retry 3` survives the intermittent flake that intermittently
  # truncates the rustup-init download. RUSTUP_AUTO_INSTALL=0 stops
  # rustup-init from trying to download *another* toolchain on top
  # of what we ask for, which can hang on slow networks.
  RUSTUP_AUTO_INSTALL=0 \
  curl --proto '=https' --tlsv1.2 -sSf --retry 3 "$url" | \
    sh -s -- -y \
      --default-toolchain "$rust_toolchain" \
      --profile minimal \
      --no-modify-path
  # Source the env file so cargo is on PATH for the rest of this
  # script. The shell rc files are NOT touched (--no-modify-path).
  # shellcheck disable=SC1091
  . "$HOME/.cargo/env"
  if ! have cargo; then
    fail "rustup install completed but cargo is still not on PATH"
    exit 1
  fi
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
      if ! have "$pkg"; then
        fail "'$pkg' could not be auto-installed"
        echo "  Install it manually and re-run (or pass --no-bootstrap)." >&2
        exit 1
      fi
      ok "$pkg installed"
    done
  else
    ok "All system packages present"
  fi

  if [ "$_need_rust" -eq 1 ]; then
    phase "Installing Rust toolchain"
    if [ -x "$HOME/.cargo/bin/rustup" ]; then
      . "$HOME/.cargo/env"
      ok "Reusing existing rustup"
    else
      install_rust
      ok "Rust ${rust_toolchain} installed"
    fi
  else
    ok "Rust toolchain present"
  fi
elif [ "$bootstrap" = "0" ]; then
  scan_dependencies
  print_dep_table
  missing=()
  for i in "${!_dep_status[@]}"; do
    if [ "${_dep_status[$i]}" = "missing" ]; then
      missing+=("${_dep_names[$i]}")
    fi
  done
  if [ ${#missing[@]} -gt 0 ]; then
    echo "" >&2
    fail "--no-bootstrap but required tools missing: ${missing[*]}"
    echo "  Install them manually and re-run." >&2
    exit 1
  fi
  ok "All dependencies satisfied"
fi

# --- final tool check ---------------------------------------------------------
missing=()
for tool in git curl make cargo rustc; do
  if ! have "$tool"; then
    missing+=("$tool")
  fi
done
if [ ${#missing[@]} -gt 0 ]; then
  echo "" >&2
  fail "required tools still missing after bootstrap: ${missing[*]}"
  echo "" >&2
  echo "  Install them manually, then re-run this script (or pass --no-bootstrap)." >&2
  exit 1
fi

# --- working directory -------------------------------------------------------
if [ -z "$workdir" ]; then
  : "${TMPDIR:=/tmp}"
  workdir="$TMPDIR/crow-install-$$"
fi

# Resolve cargo home so install target is correct.
: "${CARGO_HOME:=$HOME/.cargo}"
install_bin="$CARGO_HOME/bin/$bin_name"

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

# --- disk-space preflight (install mode only) --------------------------------
# Crow's deps pull ~150 crates. A release build needs ~600 MiB peak;
# debug ~250 MiB. On disk-quota systems this fails late with a
# cryptic "Disk quota exceeded" — fail fast with a clear number.
maybe_preflight_disk() {
  # Pick the directory we're going to build in. The workdir is set by
  # this point.
  local probe_dir="$workdir"
  if [ ! -d "$probe_dir" ]; then
    # About to clone there; probe the parent (or $TMPDIR).
    probe_dir="$(dirname "$workdir")"
  fi
  # `df -k` prints "Filesystem 1024-blocks Used Available Capacity Mounted"
  # Available is in KiB. macOS `df -k` is the same shape.
  local avail_kib
  avail_kib=$(df -k "$probe_dir" 2>/dev/null | awk 'NR==2 { print $4 }')
  if [ -z "$avail_kib" ]; then
    return 0
  fi
  # Required peak (debug ~= 250 MiB). Round up to 300 MiB to give
  # slack; warn or fail below that.
  local need_kib=$((300 * 1024))
  if [ "$avail_kib" -lt "$need_kib" ]; then
    local avail_mib=$((avail_kib / 1024))
    echo "" >&2
    fail "not enough free disk at $probe_dir"
    echo "  available: ${avail_mib} MiB" >&2
    echo "  need (debug build): ~300 MiB" >&2
    echo "  need (release build with --release): ~600 MiB" >&2
    echo "" >&2
    if [ "$yes_preflight" = "0" ]; then
      echo "  Refusing to proceed. Either:" >&2
      echo "   - free up disk space and retry" >&2
      echo "   - re-run with --no-bootstrap to bypass (cannot bypass preflight)" >&2
      echo "   - re-run with -y if you really want me to try anyway" >&2
      exit 1
    fi
    warn "continuing anyway (-y given)"
  fi
}

# --- run --------------------------------------------------------------------
run_cargo_in_step() {
  local verb="$1"
  shift
  if [ "$_verbose" = "1" ]; then
    echo "install.sh: ${verb} (cargo $*)"
    cargo "$@"
  else
    echo "install.sh: ${verb} (cargo $* ) …"
    cargo "$@" 2>&1 | tail -5
  fi
}

case "$mode" in
  test)
    phase "Running tests"
    maybe_preflight_disk
    if have make; then
      make test
    else
      run_cargo_in_step "test" test --all-targets --all-features
    fi
    ok "Tests passed"
    ;;
  install)
    maybe_preflight_disk
    if [ "$build_profile" = "release" ]; then
      phase "Building (release) and installing"
      if have make; then
        make install-release
      else
        run_cargo_in_step "install" install --path . --locked
      fi
    else
      phase "Building (debug) and installing"
      run_cargo_in_step "build" build --locked
      install -d "$CARGO_HOME/bin"
      install -m 0755 target/debug/$bin_name "$install_bin"
      if [ "$clean_after" = "1" ]; then
        cargo clean >/dev/null 2>&1 || true
      fi
    fi
    echo ""
    if [ -x "$install_bin" ]; then
      echo ""
      echo "┌─────────────────────────────────────────────┐"
      echo "│  Installation complete!                     │"
      echo "├─────────────────────────────────────────────┤"
      printf "│  path:    %-34s│\n" "$install_bin"
      printf "│  version: %-34s│\n" "$($install_bin --version 2>/dev/null || echo 'unknown')"
      printf "│  profile: %-34s│\n" "$build_profile"
      echo "└─────────────────────────────────────────────┘"
    else
      fail "$install_bin not found after install"
      exit 1
    fi
    if [ "$print_path_hint" = "1" ]; then
      case ":$PATH:" in
        *":$CARGO_HOME/bin:"*) : ;;
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
    if [ "$build_profile" = "debug" ]; then
      echo ""
      echo "  Tip: pass --release to install.sh for an optimised binary."
    fi
    ;;
esac
