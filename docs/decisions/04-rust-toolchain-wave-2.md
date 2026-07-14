---
type: decision
status: accepted
date: 2026-07-14
---

# Decision 04 — Rust toolchain pinned to 1.88 (not 1.85) for wave 2

## Context

Decision 03 pinned the toolchain to 1.85 because `genai = 0.6.5` declared `edition = "2024"`, which stabilized in Rust 1.85.

When adding `genai = 0.6.5` to Cargo.toml for wave 2, cargo reported:

```
darling_core@0.23.0 requires rustc 1.88.0
darling_macro@0.23.0 requires rustc 1.88.0
icu_collections@2.2.0 requires rustc 1.86
icu_locale_core@2.2.0 requires rustc 1.86
icu_normalizer@2.2.0 requires rustc 1.86
icu_provider@2.2.0 requires rustc 1.86
idna_adapter@1.2.2 requires rustc 1.86
serde_with@3.21.0 requires rustc 1.88
serde_with_macros@3.21.0 requires rustc 1.88
```

These are transitive dependencies of `genai`. `cargo check` failed to compile them under 1.85.

## Decision

Pin the Rust toolchain to **1.88** (not 1.85). The minimum rustc that satisfies all transitive deps is 1.88.

`rust-toolchain.toml`, `Cargo.toml` `rust-version`, and CI all become `1.88`.

## Alternatives considered

1. **Pin all transitive deps to 1.85-compatible versions.** More work, more fragile, blocks the moment a new genai release lands.
2. **Use a different `genai` version.** `genai` 0.6.5 is pinned by the spec; older versions may have different APIs and breakage.
3. **Bump the toolchain.** Chosen. The cost is one minor version bump; the win is that the entire `genai` dep graph compiles without manual pinning.

## Consequences

- Brief 1.1's `rust-version = "1.85"` is overridden. Wave 1 code still works under 1.88 (no breaking changes between minor versions).
- Future task briefs reference 1.88.
- `genai` choice is no longer blocking.
