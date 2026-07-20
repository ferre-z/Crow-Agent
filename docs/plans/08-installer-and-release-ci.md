# Plan 08 — Installer verification + release CI

**Goal:** the one-line installer actually works on clean machines, and
releases are automated.

## Current state

- `scripts/install.sh` exists (P8) — syntax-checked, NEVER RUN end-to-end.
  It bootstraps Node/pnpm, clones, installs, builds the desktop, symlinks
  `crowd`/`crow` into `~/.local/bin`.
- `apps/desktop` has an electron-builder config + `pnpm package` script —
  NEVER RUN.
- CI (`.github/workflows/ci.yml`) only runs `pnpm check`.

## Design

1. **Test the installer locally first** (don't iterate in CI):
   - In a clean container (`docker run --rm -it ubuntu:24.04 bash` or a
     fresh VM user): `curl | sh` the script from a local checkout
     (`CROW_INSTALL_REPO` + local path override), then run
     `crowd --help`, `crow --help`, `pnpm check` in the clone.
   - Fix what breaks (likely: pnpm corepack vs global install, PATH exports
     for the symlinked TS bins — they need `node` on PATH and the repo
     intact; `#!/usr/bin/env node` shebang + Node type-stripping).
2. **Release CI** (`.github/workflows/release.yml` on tags `v*`):
   - Matrix ubuntu/macos: install, `pnpm check`, `pnpm --filter
@crow/desktop package`, upload `release/` artifacts to a GitHub
     Release.
   - Also package the daemon as a tarball (`packages/` + `pnpm deploy` or a
     simple `npm pack` per package + install docs) so servers can install
     `crowd` without a full git clone.
3. **electron-builder first run**: `pnpm --filter @crow/desktop package`
   locally; fix config issues (icons, asar, workspace deps — main bundle is
   self-contained already, so no node_modules should be needed; verify
   with `--dir` unpackaged run first).

## Acceptance

- Fresh-container install ends with working `crowd`/`crow`; tag push
  produces a GitHub Release with desktop artifacts; README install
  instructions verified line-by-line.
