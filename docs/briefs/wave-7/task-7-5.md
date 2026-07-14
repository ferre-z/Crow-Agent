### Task 7.5 — Settings pane

**Files:**
- New: `crates/crow-desktop/src/frontend/components/settings-pane.ts`
- New: `src/settings.rs` (read/write `~/.config/crow/settings.toml`)
- Modify: `crates/crow-desktop/src/lib.rs` (Tauri command: `get_settings`, `set_settings`)

**Why this exists:** users want to change defaults. Approval policy, theme, telemetry, recent-project list — all live in settings.

**Spec references:** none direct — UX polish.

**Settings (the schema):**

```toml
# ~/.config/crow/settings.toml
[theme]
mode = "system"  # or "light" or "dark"

[approval]
default = "ask"  # or "allow" or "deny"
timeout_seconds = 60
remember_per_session = true

[telemetry]
enabled = false  # off by default; v0 ships no telemetry
anonymous_session_id = "..."

[notifications]
mode = "long_tasks_only"  # or "always" or "never"
threshold_seconds = 30

[recent_projects]
max_entries = 10
```

**Interfaces (exact):**

```rust
// src/settings.rs
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Settings {
    pub theme: ThemeSettings,
    pub approval: ApprovalSettings,
    pub telemetry: TelemetrySettings,
    pub notifications: NotificationSettings,
    pub recent_projects: RecentProjectSettings,
}

pub fn load() -> Settings;
pub fn save(settings: &Settings) -> Result<(), SettingsError>;
```

**Procedure:**
1. Add `toml = "0.8"` and `dirs = "5"` to Cargo.toml (already in for wave 4).
2. Implement `settings.rs` with the types and load/save.
3. Tauri commands `get_settings` and `set_settings`.
4. Settings pane UI: a tabbed interface (General / Approval / Theme / Notifications / Recent Projects / About).
5. Tests: a unit test for load/save round-trip.

**Acceptance:**
- Manual test: open Settings → change theme → it persists across restarts.
- Manual test: change approval default to "deny" → restart app → still "deny".
- `cargo build --workspace` is clean.

**Forbidden:**
- No new settings added without a default in the schema (so a fresh install has all keys set).
- No logging the settings file content (it may contain the API key path).

**Dependency:** `toml`, `dirs` already in.
