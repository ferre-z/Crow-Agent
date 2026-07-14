//! Crow is a small autonomous coding agent written in Rust.
//!
//! This crate is the v0 kernel: provider-neutral loop, tool registry,
//! JSONL session storage, cancellation, and (eventually) a Ratatui TUI.
//!
//! See `docs/waves/00-master-plan.md` in the project root for the
//! phased delivery plan, and `docs/spec/` for the v0 specification.

#![deny(rust_2018_idioms)]
#![warn(missing_debug_implementations)]

pub mod cancel;
pub mod event;
pub mod ids;
pub mod message;
pub mod provider;
pub mod session_entry;

pub use cancel::{timeout_or_cancel, CancelOutcome, CancellationToken};
pub use event::*;
pub use ids::*;
pub use message::*;
pub use session_entry::*;
