//! Crow is a small autonomous coding agent written in Rust.
//!
//! This crate is the v0 kernel: provider-neutral loop, tool registry,
//! JSONL session storage, cancellation, layered config, and a headless
//! CLI. A Tauri 2 desktop app (separate crate) consumes the same kernel
//! via the `crow serve` JSON-RPC service in a later wave.
//!
//! See `docs/waves/00-master-plan.md` in the project root for the
//! phased delivery plan. The v0 specification lives in the ob-vault
//! repository.

#![deny(rust_2018_idioms)]
#![warn(missing_debug_implementations)]

pub mod agent;
pub mod app_server;
pub mod cancel;
pub mod cli;
pub mod config;
pub mod context;
pub mod event;
pub mod ids;
pub mod mcp_opencode;
pub mod message;
pub mod policy;
pub mod provider;
pub mod session;
pub mod session_entry;
pub mod tool;

pub use cancel::{timeout_or_cancel, CancelOutcome, CancellationToken};
pub use event::*;
pub use ids::*;
pub use message::*;
pub use provider::mock::ScriptedProvider;
pub use provider::{mock, Provider};
pub use session::{SessionMeta, SessionWriter};
pub use session_entry::*;
