---
type: decision
status: accepted
date: 2026-07-14
---

# Decision 03 — Rust toolchain pinned to 1.85 (not 1.75)

## Context

Task 1.1 brief said "default to 1.75 if not documented" for the Rust toolchain pin. During task 1.2 implementation, the build failed with `feature 'edition2024' is required` because:

1. `genai = 0.6.5` (the chosen provider crate per the v0 spec §7) declares `edition = "2024"` in its Cargo.toml.
2. edition2024 is stable in Rust **1.85+** (we have 1.96.1 stable installed).
3. Our pin of 1.75 cannot parse the manifest, so cargo can't even resolve the dependency graph.

The implementer (task 1.2) discovered this empirically.

## Decision

Pin the Rust toolchain to **1.85** (not 1.75). This is the minimum version that supports edition2024, which we need because `genai 0.6.5` uses it.

`rust-toolchain.toml` becomes:
```toml
[toolchain]
channel = "1.85"
components = ["rustfmt", "clippy"]
```

`Cargo.toml` `rust-version` becomes `"1.85"`.

GitHub Actions CI uses `dtolnay/rust-toolchain@1.85`.

## Consequences

- Brief 1.1 / spec §7 is silent on the exact MSRV. This decision log is the source of truth until spec is updated.
- Future task briefs should reference 1.85 in any `rust-version` field.
- The `genai` choice is no longer blocking.
