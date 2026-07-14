### Task 1.1 — Cargo crate + CI

**Files:**
- Create: `Cargo.toml`, `rust-toolchain.toml`, `clippy.toml`, `.github/workflows/ci.yml`, `.cargo/config.toml`
- Create: `src/main.rs` (binary; on `crow --version` or no args, prints "crow 0.1.0" and exits 0)
- Create: `src/lib.rs` (re-exports nothing yet, just exists so integration tests can import)

**Spec references:** v0 spec §6 (project structure — one binary crate, NOT a workspace), §7 (dependencies), §15 (CLI).

**CRITICAL spec constraints:**
- The root `Cargo.toml` is a **single binary crate**, not a `[workspace]`. A workspace contradicts spec §6 ("The initial repository is **one binary crate**. Create library crates only after a second application needs to embed the agent core.").
- `rust-toolchain.toml` pins to the **MSRV of `genai` 0.6.5**. Default to 1.75 if not documented. CI must use this exact pinned toolchain, NOT `stable`.
- `clippy.toml` enables individual lints. Lint GROUPS (like `pedantic`, `restriction`) go in `[lints.clippy]` in `Cargo.toml`, NOT in `clippy.toml`.

**Cargo.toml `[dependencies]` (required for tasks 1.2 and 1.5 to compile day one):**
```toml
[package]
name = "crow"
version = "0.1.0"
edition = "2021"
rust-version = "1.75"
description = "Small autonomous coding agent"
license = "MIT"

[lib]
name = "crow"
path = "src/lib.rs"

[[bin]]
name = "crow"
path = "src/main.rs"

[lints.clippy]
pedantic = { level = "warn", priority = -1 }
# restriction = ... is intentionally NOT enabled — would block std::fs paths

[dependencies]
tokio = { version = "1", features = ["macros", "rt-multi-thread", "sync", "time", "fs", "process", "io-util"] }
tokio-util = { version = "0.7", features = ["rt"] }
ulid = { version = "1", features = ["serde"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "1"
async-trait = "0.1"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "fmt"] }
tempfile = "3"

[dev-dependencies]
tokio = { version = "1", features = ["macros", "rt-multi-thread", "test-util"] }
```

> **No `chrono` in this task.** The reviewer flagged Decision 02 as REJECT (the `clock` feature pulls in `iana-time-zone` anyway, so the "consequence" was false). Tasks that need timestamps in 1.2 use `std::time::SystemTime` wrapped in a project-owned `Timestamp` newtype with manual `serde::Serialize/Deserialize`. A `time` crate dependency can be added in wave 2 if needed.

**Timestamp newtype lives where?**
- For wave 1, define `pub struct Timestamp(pub std::time::SystemTime);` with `#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]` and a hand-written `Serialize/Deserialize` that emits `{"unix_ms": <u64>}` (computed via `SystemTime::duration_since(UNIX_EPOCH)`). Place it in `src/ids.rs` next to the ID types — it's a small primitive.
- This is the **approved** approach for v0. Decision 02 is RESCINDED.

**GitHub Actions CI (`.github/workflows/ci.yml`):**
- Triggers: push to main + all PRs
- Runs on `ubuntu-latest`
- Steps: checkout → `dtolnay/rust-toolchain@<pinned version from rust-toolchain.toml>` (use `dtolnay/rust-toolchain` action's `toolchain:` key matching the pin) → run the gate
- Caches `~/.cargo/registry` and `target/`

**Acceptance:**
- `cargo build --release` exits 0
- `cargo fmt --all --check` exits 0
- `cargo clippy --all-targets --all-features -- -D warnings` exits 0
- `cargo test` exits 0 (no tests yet, just runs)
- `crow --version` prints `crow 0.1.0` and exits 0
- `crow` (no args) prints `crow 0.1.0` and exits 0
- CI workflow file is valid YAML and uses the **pinned toolchain** (not stable)
- `Cargo.toml` does NOT contain `[workspace]`
- `clippy.toml` does NOT contain `[lints]` (those are in `Cargo.toml`)

**Forbidden:** No `[workspace]`. No `unsafe`. No git hooks. No release config. No business logic. No `chrono`.
