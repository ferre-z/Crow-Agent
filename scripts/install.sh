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

# Parse flags.
while [ $# -gt 0 ]; do
  case "$1" in
    --test)           mode="test"; shift ;;
    --release)        build_profile="release"; clean_after=0; shift ;;
    --path)           workdir="${2:-}"; shift 2 || { echo "install.sh: --path requires an argument" >&2; exit 1; } ;;
    --path=*)         workdir="${1#*=}"; shift ;;
    --branch)         branch="${2:-}"; shift 2 || { echo "install.sh: --branch requires an argument" >&2; exit 1; } ;;
    --branch=*)       branch="${1#*=}"; shift ;;
    --repo)           repo="${2:-}"; shift 2 || { echo "install.sh: --repo requires an argument" >&2; exit 1; } ;;
    --repo=*)         repo="${1#*=}"; shift ;;
    --no-bootstrap)   bootstrap=0; shift ;;
    --no-path-hint)   print_path_hint=0; shift ;;
    --no-clean)       clean_after=0; shift ;;
    -y|--yes)         yes_preflight=1; shift ;;
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

# Install Rust via rustup. Uses curl (which we ensured above).
install_rust() {
  echo "install.sh: installing Rust toolchain via rustup (channel ${rust_toolchain})"
  local url="https://sh.rustup.rs"
  curl --proto '=https' --tlsv1.2 -sSf "$url" | \
    sh -s -- -y \
      --default-toolchain "$rust_toolchain" \
      --profile minimal \
      --no-modify-path
  # Source the env file so cargo is on PATH for the rest of this
  # script. The shell rc files are NOT touched (--no-modify-path).
  # shellcheck disable=SC1091
  . "$HOME/.cargo/env"
  if ! have cargo; then
    echo "install.sh: rustup install completed but cargo is still not on PATH" >&2
    exit 1
  fi
}

# --- bootstrap ---------------------------------------------------------------
if [ "$bootstrap" = "1" ]; then
  # Step 1: install curl first because rustup needs it.
  install_packages curl
  if ! have curl; then
    echo "install.sh: 'curl' is required but could not be auto-installed." >&2
    echo "  Install curl manually (e.g. apt install curl / brew install curl)" >&2
    echo "  and re-run this script." >&2
    exit 1
  fi

  # Step 2: install Rust if missing. This is the biggest win — most
  # users won't have cargo on a fresh box.
  if ! have cargo || ! have rustc; then
    install_rust
  else
    # Cargo exists. Make sure the pinned toolchain is available; rustup
    # auto-installs from rust-toolchain.toml when invoked.
    if have rustup && [ -f rust-toolchain.toml ]; then
      # No-op: rustup itself honours rust-toolchain.toml per-directory.
      :
    fi
  fi

  # Step 3: install git and make (and curl is now confirmed above).
  install_packages git make
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
  echo "install.sh: required tools still missing after bootstrap: ${missing[*]}" >&2
  echo "" >&2
  echo "  Install them manually, then re-run this script (or pass --no-bootstrap" >&2
  echo "  to suppress auto-install entirely)." >&2
  echo "" >&2
  case "$uname_s" in
    Linux)
      echo "  Linux: 'sudo apt-get install git make curl' (Debian/Ubuntu)" >&2
      echo "         'sudo dnf install git make curl'    (Fedora/RHEL)" >&2
      echo "         'sudo pacman -S git make curl'      (Arch)" >&2
      ;;
    Darwin)
      echo "  macOS: 'xcode-select --install'              (Apple toolchain + git)" >&2
      echo "         'brew install make curl'              (or use Xcode CLT)" >&2
      ;;
  esac
  echo "" >&2
  echo "  For Rust on any platform: see https://rustup.rs" >&2
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

echo "install.sh: target directory: $workdir"

# Reuse existing checkout (idempotent) when present.
if [ ! -d "$workdir/.git" ]; then
  echo "install.sh: cloning $repo (branch $branch) — depth 1"
  git clone --depth 1 --branch "$branch" "$repo" "$workdir"
else
  echo "install.sh: reusing existing checkout at $workdir"
  (cd "$workdir" && git fetch --depth 1 origin "$branch" && git checkout "$branch" && git reset --hard "origin/$branch") \
    || echo "install.sh: warning: could not fast-forward existing checkout; continuing with current tree"
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
    echo "install.sh: not enough free disk at $probe_dir" >&2
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
    echo "install.sh: continuing anyway (-y given)" >&2
  fi
}

# --- run --------------------------------------------------------------------
run_cargo_in_step() {
  # $1 = sub-step verb, $2.. = cargo args
  local verb="$1"
  shift
  echo "install.sh: ${verb} (cargo $* )"
  cargo "$@"
}

case "$mode" in
  test)
    maybe_preflight_disk
    echo "install.sh: running test suite"
    if have make; then
      make test
    else
      # Defensive fallback — bootstrap should have installed make.
      echo "install.sh: 'make' missing; falling back to direct cargo test"
      run_cargo_in_step "test" test --all-targets --all-features
    fi
    echo "install.sh: tests passed"
    ;;
  install)
    maybe_preflight_disk
    if [ "$build_profile" = "release" ]; then
      echo "install.sh: building (release profile) and installing to $install_bin"
      if have make; then
        make install-release
      else
        # 'cargo install' defaults to release build.
        run_cargo_in_step "install" install --path . --locked
      fi
    else
      echo "install.sh: building (debug profile) and installing to $install_bin"
      # Debug build is much smaller on disk and fits on quota-limited
      # boxes. We copy the binary to ~/.cargo/bin and then `cargo
      # clean` so the build artifacts (~250 MiB) don't stick around.
      run_cargo_in_step "build" build --locked
      install -d "$CARGO_HOME/bin"
      install -m 0755 target/debug/$bin_name "$install_bin"
      if [ "$clean_after" = "1" ]; then
        echo "install.sh: cleaning build artifacts"
        cargo clean >/dev/null 2>&1 || true
      fi
    fi
    echo ""
    echo "install.sh: installed."
    if [ -x "$install_bin" ]; then
      echo "  path: $install_bin"
      echo "  version: $($install_bin --version)"
      echo "  profile: $build_profile"
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
    if [ "$build_profile" = "debug" ]; then
      echo ""
      echo "  Tip: pass --release to install.sh for an optimised binary."
    fi
    ;;
esac
