//! CLI smoke tests for the `crow tui` subcommand.
//!
//! The TUI itself is interactive and hard to drive from `assert_cmd`
//! directly, but we can at least confirm the subcommand is wired
//! into clap, that `--help` renders, and that bad flags fail with a
//! clap-formatted error.
//!
//! Round-trip rendering is covered manually via `script(1)` in the
//! TUI driver's own comment block; this file only owns the static
//! subcommand surface.

use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn tui_subcommand_is_listed_in_help() {
    Command::cargo_bin("crow")
        .unwrap()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("tui"));
}

#[test]
fn tui_help_describes_resume_flag() {
    Command::cargo_bin("crow")
        .unwrap()
        .args(["tui", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--resume"));
}

#[test]
fn tui_unknown_subflag_is_rejected() {
    // clap should reject bogus flags without spinning up the TUI.
    Command::cargo_bin("crow")
        .unwrap()
        .args(["tui", "--definitely-not-a-flag"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("error"));
}
