#!/usr/bin/env bash
# env-doctor.sh — diagnostic the user runs on their box so I can
# design install/runtime code against real numbers instead of guessing.
#
# Output is plain text, ~30 lines. Paste it back when ready.
#
# Does NOT change anything on disk. Read-only.

set -e

echo "=== uname ==="
uname -a
echo
echo "=== distro ==="
[ -f /etc/os-release ] && grep -E '^(NAME|VERSION|ID)=' /etc/os-release || echo "(no /etc/os-release)"
echo
echo "=== disk on /, /tmp, /home ==="
df -h / /tmp /home 2>&1 | head -10
echo
echo "=== memory + swap ==="
free -h 2>&1 | head -5
echo
echo "=== mount points for /, /tmp, /home ==="
mount | grep -E ' on /(home|tmp)( |$)' || true
echo
echo "=== user quota (Linux quota-tools) ==="
quota -v 2>&1 | head -10 || echo "(quota-tools not installed)"
echo
echo "=== already-installed toolchain footprint ==="
du -sh ~/.cargo ~/.rustup ~/.local ~/.cache 2>/dev/null
echo
echo "=== installed Rust + build tools ==="
for t in cargo rustc rustup git make curl sudo sccache mold; do
  if command -v "$t" >/dev/null 2>&1; then
    printf '  %-8s -> %s\n' "$t" "$(command -v "$t")"
  else
    printf '  %-8s (missing)\n' "$t"
  fi
done
echo
echo "=== opencode / bun state (helps diagnose the TUI crash) ==="
which opencode 2>&1 || echo "(opencode missing)"
which bun 2>&1 || echo "(bun missing)"
ls -la ~/.bun/install/cache 2>/dev/null | head -10 || echo "(no ~/.bun/install/cache)"
find ~ -name 'libopentui*.so' 2>/dev/null | head -5 || echo "(no libopentui*.so anywhere under HOME)"
echo
echo "=== done. paste this output back. ==="
