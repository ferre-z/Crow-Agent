//! Slash-command parsing.
//!
//! Slash commands (`/help`, `/quit`, `/clear`, `/doctor`, `/model`)
//! are intercepted by the TUI driver before any text reaches the
//! agent. This module owns the parser — `parse_slash` returns a
//! [`SlashOutcome`] the driver pattern-matches on.
//!
//! Adding a new command? Add it to [`parse_slash`] and to
//! [`crate::tui::app::SLASH_HELP`] so `/help` stays truthful.

use std::str::FromStr;

/// What the TUI driver should do with a parsed slash command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlashOutcome {
    /// Pure UI side-effect (`/clear`, `/help`, `/doctor`, `/model`,
    /// `/resume`). The driver dispatches `name` + `args` to
    /// [`crate::tui::app::App::apply_local_slash`].
    Local { name: String, args: String },
    /// Submit `text` as the next agent prompt. Used for the rare
    /// slash command that *does* need the agent (none in v1, but
    /// the slot is reserved for `/summarize` and friends).
    Submit(String),
    /// Quit the TUI.
    Quit,
}

/// Classify `input` as either a plain prompt or a slash command.
///
/// Returns `None` if the input is not a slash command at all (the
/// caller sends it to the agent unchanged). Leading whitespace is
/// tolerated so users can write ` /help` and have it still count.
pub fn parse_slash(input: &str) -> Option<SlashOutcome> {
    let trimmed = input.trim_start();
    let mut chars = trimmed.chars();
    if chars.next() != Some('/') {
        return None;
    }
    let body = &trimmed[1..];
    let (name, args) = match body.find(char::is_whitespace) {
        Some(idx) => (&body[..idx], body[idx..].trim_start()),
        None => (body, ""),
    };
    let outcome = SlashCommand::from_str(name).ok()?.outcome(args);
    Some(outcome)
}

/// Enumerated slash commands. Add new variants here, then handle
/// them in [`crate::tui::app::App::apply_local_slash`] (for
/// `Local` effects) or in the TUI driver (for `Quit` / async).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)] // some variants are matched in FromStr, not constructed directly
enum SlashCommand {
    Help,
    Clear,
    Quit,
    Exit,
    Doctor,
    Model,
    Resume,
    Plan,
    Cost,
    Status,
    AddDir,
}

impl SlashCommand {
    /// Map the command to the action the driver should take.
    fn outcome(self, args: &str) -> SlashOutcome {
        let name = match self {
            SlashCommand::Help => "help",
            SlashCommand::Clear => "clear",
            SlashCommand::Doctor => "doctor",
            SlashCommand::Model => "model",
            SlashCommand::Resume => "resume",
            SlashCommand::Plan => "plan",
            SlashCommand::Cost => "cost",
            SlashCommand::Status => "status",
            SlashCommand::AddDir => "add-dir",
            SlashCommand::Quit | SlashCommand::Exit => return SlashOutcome::Quit,
        };
        SlashOutcome::Local {
            name: name.to_string(),
            args: args.to_string(),
        }
    }
}

impl FromStr for SlashCommand {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "help" | "?" => Self::Help,
            "clear" | "cls" => Self::Clear,
            "quit" | "exit" | "q" => Self::Quit,
            "doctor" => Self::Doctor,
            "model" => Self::Model,
            "resume" => Self::Resume,
            "plan" => Self::Plan,
            "cost" => Self::Cost,
            "status" => Self::Status,
            "add-dir" | "adddir" => Self::AddDir,
            _ => return Err(()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_slash_returns_none() {
        assert!(parse_slash("hello").is_none());
        assert!(parse_slash("").is_none());
        assert!(parse_slash("hello /help").is_none()); // slash mid-line is not a command
    }

    #[test]
    fn slash_quit_recognised() {
        assert_eq!(parse_slash("/quit"), Some(SlashOutcome::Quit));
        assert_eq!(parse_slash("/exit"), Some(SlashOutcome::Quit));
        assert_eq!(parse_slash("/q"), Some(SlashOutcome::Quit));
    }

    #[test]
    fn slash_local_commands() {
        assert_eq!(
            parse_slash("/help"),
            Some(SlashOutcome::Local {
                name: "help".into(),
                args: String::new(),
            })
        );
        assert_eq!(
            parse_slash("/clear"),
            Some(SlashOutcome::Local {
                name: "clear".into(),
                args: String::new(),
            })
        );
        assert_eq!(
            parse_slash("/resume 01ABC"),
            Some(SlashOutcome::Local {
                name: "resume".into(),
                args: "01ABC".into(),
            })
        );
    }

    #[test]
    fn unknown_slash_is_none() {
        // Unknown slash commands fall through and get sent to the
        // agent (so the model can react to typos naturally).
        assert!(parse_slash("/frobnicate").is_none());
    }

    #[test]
    fn leading_whitespace_tolerated() {
        assert_eq!(parse_slash("   /quit"), Some(SlashOutcome::Quit));
        assert_eq!(
            parse_slash("\t/help"),
            Some(SlashOutcome::Local {
                name: "help".into(),
                args: String::new(),
            })
        );
    }

    #[test]
    fn args_are_split_on_whitespace() {
        // The parser ignores args for outcome classification; the
        // /resume handler in the app uses them. We just verify the
        // split works.
        let trimmed = "/resume 01ABCDEF".trim_start();
        let body = &trimmed[1..];
        let (name, args) = match body.find(char::is_whitespace) {
            Some(idx) => (&body[..idx], body[idx..].trim_start()),
            None => (body, ""),
        };
        assert_eq!(name, "resume");
        assert_eq!(args, "01ABCDEF");
    }
}
