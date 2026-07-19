//! Session picker overlay state.
//!
//! Pure state machine: no rendering, no async, no event-channel
//! plumbing. The TUI driver loads a list of [`PickerEntry`] rows
//! (typically via [`crate::session::list_sessions`]), drops them
//! into a [`SessionPicker`], and lets the user navigate with
//! arrow keys / PgUp/PgDn / Home/End. The overlay reports the
//! chosen entry back through [`SessionPicker::selected`] so the
//! driver can act on it (today: print the resume command and exit;
//! later slices: tear down the worker and rebuild the agent in
//! place).
//!
//! Keeping the picker state in its own module — rather than in
//! `app.rs` — keeps `App`'s struct small and lets the picker be
//! tested in isolation.

/// One row in the session picker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PickerEntry {
    /// 26-char ULID, displayed in full and matched on prefix.
    pub session_id: String,
    /// Human-readable start timestamp (UTC, `YYYY-MM-DDTHH:MM:SSZ`).
    pub started_at: String,
    /// Tail of the on-disk path, for orientation.
    pub path_tail: String,
}

/// Session picker state.
///
/// `selected` is the index into `entries`. `scroll` is the row at
/// the top of the viewport (used by the renderer to keep the
/// highlight on screen).
#[derive(Debug, Clone)]
pub struct SessionPicker {
    /// All picker rows, in display order (newest first).
    entries: Vec<PickerEntry>,
    /// Highlighted index. Always in `[0, entries.len())` once the
    /// picker has been opened with at least one entry.
    selected: usize,
    /// Top-of-viewport row index.
    scroll: usize,
}

/// Result of a navigation action — the picker mutates itself and
/// returns a hint about what happened (purely informational, the
/// caller can ignore it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickerAction {
    /// `selected` index moved.
    Moved,
    /// `selected` index unchanged (already at the boundary).
    Clamped,
}

impl SessionPicker {
    /// Open a picker over `entries`. If the list is empty, `selected`
    /// stays at 0 — the renderer is responsible for showing an
    /// "empty" placeholder.
    #[must_use]
    pub fn new(entries: Vec<PickerEntry>) -> Self {
        Self {
            entries,
            selected: 0,
            scroll: 0,
        }
    }

    /// Number of entries in the picker.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// True if the picker has no rows to show.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Index of the highlighted entry.
    #[must_use]
    pub fn selected_index(&self) -> usize {
        self.selected
    }

    /// Top-of-viewport row index.
    #[must_use]
    pub fn scroll(&self) -> usize {
        self.scroll
    }

    /// Borrow the row at `index`, or `None` if out of range.
    #[must_use]
    pub fn get(&self, index: usize) -> Option<&PickerEntry> {
        self.entries.get(index)
    }

    /// Highlight the row the user just selected, if any.
    #[must_use]
    pub fn selected(&self) -> Option<&PickerEntry> {
        self.entries.get(self.selected)
    }

    /// Move the highlight down by one row. Returns [`PickerAction::Clamped`]
    /// when already at the bottom (no movement).
    pub fn select_next(&mut self) -> PickerAction {
        if self.entries.is_empty() {
            return PickerAction::Clamped;
        }
        if self.selected + 1 < self.entries.len() {
            self.selected += 1;
            PickerAction::Moved
        } else {
            PickerAction::Clamped
        }
    }

    /// Move the highlight up by one row.
    pub fn select_prev(&mut self) -> PickerAction {
        if self.selected > 0 {
            self.selected -= 1;
            PickerAction::Moved
        } else {
            PickerAction::Clamped
        }
    }

    /// Page down by `page` rows (used by PgDn).
    pub fn page_down(&mut self, page: usize) -> PickerAction {
        if self.entries.is_empty() {
            return PickerAction::Clamped;
        }
        let target = self.selected.saturating_add(page);
        let target = target.min(self.entries.len() - 1);
        if target == self.selected {
            PickerAction::Clamped
        } else {
            self.selected = target;
            PickerAction::Moved
        }
    }

    /// Page up by `page` rows.
    pub fn page_up(&mut self, page: usize) -> PickerAction {
        let target = self.selected.saturating_sub(page);
        if target == self.selected {
            PickerAction::Clamped
        } else {
            self.selected = target;
            PickerAction::Moved
        }
    }

    /// Jump to the first row.
    pub fn select_first(&mut self) -> PickerAction {
        if self.selected == 0 {
            PickerAction::Clamped
        } else {
            self.selected = 0;
            PickerAction::Moved
        }
    }

    /// Jump to the last row.
    pub fn select_last(&mut self) -> PickerAction {
        if self.entries.is_empty() {
            return PickerAction::Clamped;
        }
        let last = self.entries.len() - 1;
        if self.selected == last {
            PickerAction::Clamped
        } else {
            self.selected = last;
            PickerAction::Moved
        }
    }

    /// Update the viewport scroll so the highlighted row is on
    /// screen. `viewport` is the number of rows the renderer shows.
    ///
    /// The renderer calls this on every redraw, so the highlight
    /// never escapes the viewport.
    pub fn ensure_visible(&mut self, viewport: usize) {
        if self.entries.is_empty() || viewport == 0 {
            self.scroll = 0;
            return;
        }
        // Highlight above the viewport — scroll up.
        if self.selected < self.scroll {
            self.scroll = self.selected;
            return;
        }
        // Highlight below the viewport — scroll down.
        if self.selected >= self.scroll + viewport {
            self.scroll = self.selected + 1 - viewport;
            return;
        }
        // Clamp to valid bounds (in case entries shrank).
        let max_scroll = self.entries.len().saturating_sub(viewport);
        if self.scroll > max_scroll {
            self.scroll = max_scroll;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entries(n: usize) -> Vec<PickerEntry> {
        (0..n)
            .map(|i| PickerEntry {
                session_id: format!("01ABCDEFGHJKMNPQRSTVWXYZ{i:02}"),
                started_at: format!("2026-07-18T15:00:0{i}Z"),
                path_tail: format!("/tmp/sess{i}.jsonl"),
            })
            .collect()
    }

    #[test]
    fn new_picker_starts_at_zero() {
        let p = SessionPicker::new(entries(3));
        assert_eq!(p.selected_index(), 0);
        assert_eq!(p.scroll(), 0);
    }

    #[test]
    fn empty_picker_handles_navigation() {
        let mut p = SessionPicker::new(Vec::new());
        assert!(p.is_empty());
        assert_eq!(p.select_next(), PickerAction::Clamped);
        assert_eq!(p.select_prev(), PickerAction::Clamped);
        assert_eq!(p.page_down(10), PickerAction::Clamped);
        assert_eq!(p.page_up(10), PickerAction::Clamped);
        assert_eq!(p.select_first(), PickerAction::Clamped);
        assert_eq!(p.select_last(), PickerAction::Clamped);
    }

    #[test]
    fn select_next_moves_down_and_clamps_at_end() {
        let mut p = SessionPicker::new(entries(3));
        assert_eq!(p.select_next(), PickerAction::Moved);
        assert_eq!(p.selected_index(), 1);
        assert_eq!(p.select_next(), PickerAction::Moved);
        assert_eq!(p.selected_index(), 2);
        assert_eq!(p.select_next(), PickerAction::Clamped);
        assert_eq!(p.selected_index(), 2);
    }

    #[test]
    fn select_prev_moves_up_and_clamps_at_start() {
        let mut p = SessionPicker::new(entries(3));
        p.select_last();
        assert_eq!(p.selected_index(), 2);
        assert_eq!(p.select_prev(), PickerAction::Moved);
        assert_eq!(p.selected_index(), 1);
        assert_eq!(p.select_prev(), PickerAction::Moved);
        assert_eq!(p.selected_index(), 0);
        assert_eq!(p.select_prev(), PickerAction::Clamped);
        assert_eq!(p.selected_index(), 0);
    }

    #[test]
    fn page_down_jumps_by_page_and_clamps_at_end() {
        let mut p = SessionPicker::new(entries(10));
        assert_eq!(p.page_down(4), PickerAction::Moved);
        assert_eq!(p.selected_index(), 4);
        assert_eq!(p.page_down(10), PickerAction::Moved);
        assert_eq!(p.selected_index(), 9);
        assert_eq!(p.page_down(1), PickerAction::Clamped);
    }

    #[test]
    fn page_up_jumps_by_page_and_clamps_at_start() {
        let mut p = SessionPicker::new(entries(10));
        p.select_last();
        assert_eq!(p.page_up(3), PickerAction::Moved);
        assert_eq!(p.selected_index(), 6);
        assert_eq!(p.page_up(100), PickerAction::Moved);
        assert_eq!(p.selected_index(), 0);
        assert_eq!(p.page_up(1), PickerAction::Clamped);
    }

    #[test]
    fn first_and_last_jumps() {
        let mut p = SessionPicker::new(entries(5));
        assert_eq!(p.select_last(), PickerAction::Moved);
        assert_eq!(p.selected_index(), 4);
        assert_eq!(p.select_first(), PickerAction::Moved);
        assert_eq!(p.selected_index(), 0);
        assert_eq!(p.select_first(), PickerAction::Clamped);
        assert_eq!(p.select_last(), PickerAction::Moved);
        assert_eq!(p.select_last(), PickerAction::Clamped);
    }

    #[test]
    fn ensure_visible_scrolls_when_highlight_leaves_viewport() {
        let mut p = SessionPicker::new(entries(10));
        // Viewport of 3. Highlight row 7 — need scroll = 5.
        for _ in 0..7 {
            p.select_next();
        }
        p.ensure_visible(3);
        assert_eq!(p.selected_index(), 7);
        assert_eq!(p.scroll(), 5);
        // Scroll up — highlight row 0, scroll back to 0.
        for _ in 0..7 {
            p.select_prev();
        }
        p.ensure_visible(3);
        assert_eq!(p.selected_index(), 0);
        assert_eq!(p.scroll(), 0);
    }

    #[test]
    fn ensure_visible_does_not_scroll_when_in_viewport() {
        let mut p = SessionPicker::new(entries(10));
        p.select_next();
        p.select_next();
        p.ensure_visible(5);
        assert_eq!(p.scroll(), 0);
    }

    #[test]
    fn selected_returns_highlighted_entry() {
        let p = SessionPicker::new(entries(3));
        let entry = p.selected().expect("at least one");
        assert!(entry.session_id.starts_with("01ABCDEFGHJKMNPQRSTVWXYZ"));
    }

    #[test]
    fn picker_entry_equality() {
        let a = PickerEntry {
            session_id: "01ABC".into(),
            started_at: "2026-07-18T15:00:00Z".into(),
            path_tail: "/tmp/x.jsonl".into(),
        };
        let b = a.clone();
        assert_eq!(a, b);
    }
}
