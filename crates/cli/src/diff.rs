//! Renders a before and after as diff lines. The model never diffs anything, so this is
//! the one place that decides what a change looks like.

use similar::{ChangeTag, TextDiff};
use std::fmt::Write as _;

pub const RED: &str = "\x1b[31m";
pub const GREEN: &str = "\x1b[32m";
pub const DIM: &str = "\x1b[2m";
pub const BOLD: &str = "\x1b[1m";
pub const RESET: &str = "\x1b[0m";

pub struct Paint {
    on: bool,
}

impl Paint {
    /// Honours NO_COLOR, and stays off when not on a terminal.
    pub fn new(terminal: bool) -> Self {
        Self {
            on: terminal && std::env::var_os("NO_COLOR").is_none(),
        }
    }

    pub fn wrap(&self, colour: &str, text: &str) -> String {
        if self.on {
            format!("{colour}{text}{RESET}")
        } else {
            text.to_string()
        }
    }
}

/// Unchanged lines around an edit, so a body can be read without its whole context.
const CONTEXT: usize = 2;

pub fn render(before: &str, after: &str, paint: &Paint) -> String {
    let diff = TextDiff::from_lines(before, after);
    let mut out = String::new();

    for (group, changes) in diff.grouped_ops(CONTEXT).iter().enumerate() {
        if group > 0 {
            let _ = writeln!(out, "{}", paint.wrap(DIM, "    ..."));
        }
        for op in changes {
            for change in diff.iter_changes(op) {
                let text = change.value().trim_end_matches('\n');
                let (mark, colour) = match change.tag() {
                    ChangeTag::Delete => ('-', RED),
                    ChangeTag::Insert => ('+', GREEN),
                    ChangeTag::Equal => (' ', DIM),
                };
                let _ = writeln!(out, "  {}", paint.wrap(colour, &format!("{mark} {text}")));
            }
        }
    }

    out
}

/// For a definition that didn't change but has to be read anyway.
pub fn render_unchanged(text: &str, paint: &Paint) -> String {
    text.lines()
        .map(|line| format!("  {}\n", paint.wrap(DIM, &format!("  {line}"))))
        .collect()
}

/// A whole text arriving or leaving, where every line is the change.
pub fn render_whole(text: &str, mark: char, paint: &Paint) -> String {
    let colour = if mark == '+' { GREEN } else { RED };
    text.lines()
        .map(|line| format!("  {}\n", paint.wrap(colour, &format!("{mark} {line}"))))
        .collect()
}
