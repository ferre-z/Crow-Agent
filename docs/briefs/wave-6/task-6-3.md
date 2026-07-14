### Task 6.3 — OS keyring for API keys

**Files:**
- New: `src/keyring.rs` (Rust wrapper around the `keyring` crate)
- Modify: `src/server/handlers.rs` (use the keyring for the API key)
- New: `src/cli.rs` (the `crow login` and `crow logout` subcommands)
- Modify: `crates/crow-desktop/src/lib.rs` (Tauri command: `set_api_key`)

**Why this exists:** users should not paste API keys into plaintext config files. Use the OS's native credential store (Keychain on macOS, Credential Manager on Windows, Secret Service on Linux).

**Spec references:** spec §4 (the API key must come from the configured env var, "NVIDIA_API_KEY"). The keyring is a layered alternative — read from keyring first, fall back to env var.

**Behavior:**
- `crow login` prompts for the API key (without echoing). Stores it in the OS keyring under service `crow`, account `nvidia-api-key`.
- `crow logout` removes the entry.
- `crow serve` and the desktop app read from the keyring first. If missing, fall back to `NVIDIA_API_KEY` env var. If both missing, error.

**Interfaces (exact):**

```rust
// src/keyring.rs
pub fn get_api_key() -> Result<Option<String>, KeyringError>;
pub fn set_api_key(key: &str) -> Result<(), KeyringError>;
pub fn delete_api_key() -> Result<(), KeyringError>;

#[derive(Debug, thiserror::Error)]
pub enum KeyringError {
    #[error("keyring backend error: {0}")] Backend(String),
    #[error("no entry found")] NoEntry,
}
```

```rust
// In src/cli.rs
#[derive(Subcommand)]
pub enum Subcommand {
    // ... existing
    Login,    // prompts for API key
    Logout,   // removes the key
}
```

**Procedure:**
1. Add `keyring = "3"` to Cargo.toml.
2. Implement `keyring.rs` with the three functions.
3. Add `Login` and `Logout` subcommands. `Login` uses `rpassword` (or `dialoguer`) to prompt without echo.
4. Modify `handle_session_start` to use the keyring before the env var.
5. Modify the desktop's Tauri command `set_api_key` to write to the keyring (and trigger a reload).
6. Tests: a unit test that uses the `keyring::mock` crate to verify the read/write/delete cycle.

**Acceptance:**
- `crow login` prompts without echo and stores the key.
- `crow logout` removes the key.
- `crow serve` reads from the keyring.
- `crow doctor` reports whether the key is present.
- A desktop "Sign in" button calls `set_api_key` and the next request uses it.
- `cargo build --workspace` is clean.

**Forbidden:**
- No printing the API key to logs (redact everywhere).
- No plaintext config files (the keyring is the only place).
- No writing the key to a file in the project (security risk).

**Dependency:** `keyring = "3"`, `rpassword = "7"` (or `dialoguer` for prompting).
