### Task 5.8 — Native packaging

**Files:**
- Modify: `crates/crow-desktop/tauri.conf.json` (bundle config)
- New: `crates/crow-desktop/build.rs` (icons, version bump)
- New: `crates/crow-desktop/.github/workflows/release.yml` (CI for tagged releases)

**Why this exists:** "works on my machine" is not enough. The desktop app must produce signed installers for macOS, Windows, and Linux.

**Targets:**
- **macOS:** `.app` bundle, `.dmg` installer. Code-signed with a Developer ID (development cert is fine for v0).
- **Windows:** `.msi` installer. Signed with a code-signing cert (defer signing to v1 if not available).
- **Linux:** `.deb` for Debian/Ubuntu, `.AppImage` for everything else.

**Procedure:**
1. Add icon assets: `crates/crow-desktop/icons/icon.png` + the various `.icns` / `.ico` formats. Use a simple placeholder (a "C" on a yellow background — matches DesignerAtWork brand from the vault).
2. Configure `tauri.conf.json`:
   ```json
   {
     "bundle": {
       "active": true,
       "targets": ["app", "dmg", "msi", "deb", "appimage"],
       "icon": ["icons/32x32.png", "icons/128x128.png", "icons/icon.icns", "icons/icon.ico"],
       "category": "DeveloperTool",
       "shortDescription": "Small autonomous coding agent",
       "longDescription": "..."
     }
   }
   ```
3. Set up signing on macOS (development cert) and Windows (skip for v0).
4. Add a `cargo tauri build` invocation in CI on tag push.
5. Manual smoke test: build the installers on each platform, install, launch, see the splash screen.

**Acceptance:**
- `cargo tauri build` produces all 4 installers on the host platform.
- The macOS `.app` launches without "unidentified developer" warnings.
- The Windows `.msi` installs without UAC prompts (if signed).
- The Linux `.AppImage` runs on a clean Ubuntu 22.04.

**Forbidden:**
- No production code-signing in v0 (dev certs are fine).
- No App Store / Microsoft Store submission in v0 (just direct downloads).
- No auto-updater (defer to v1).

**Dependency:** `tauri = "2"` already in.
