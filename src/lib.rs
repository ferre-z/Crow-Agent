//! Crow is a small autonomous coding agent written in Rust.
//!
//! This crate is the v0 kernel: provider-neutral loop, tool registry,
//! JSONL session storage, cancellation, and (eventually) a Ratatui TUI.
//!
//! See `docs/waves/00-master-plan.md` in the project root for the
//! phased delivery plan, and `docs/spec/` for the v0 specification.

#![deny(rust_2018_idioms)]
#![warn(missing_debug_implementations)]
