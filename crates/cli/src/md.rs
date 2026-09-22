//! `dagger md`: the review as markdown, for an agent or a pull request description. The
//! reading order with what each step depends on and its diff, so a reader with no page
//! still gets the story in the order it should be read.

use crate::diff::{self, Paint};
use crate::{report, walk};
use dagger_core::change::Mark;
use dagger_core::model::Identity;
use dagger_core::review::Review;
use std::collections::BTreeMap;
use std::fmt::Write as _;

pub fn render(review: &Review) -> String {
    let names: BTreeMap<Identity, String> = review
        .definitions
        .iter()
        .map(|(identity, definition)| (*identity, report::name(definition)))
        .collect();
    let named = |identity: &Identity| {
        names
            .get(identity)
            .map(|name| walk::short(name))
            .unwrap_or_else(|| "?".to_string())
    };
    let plain = Paint::new(false);

    let mut out = String::new();
    let _ = writeln!(out, "# {}\n", review.title.as_deref().unwrap_or("Review"));
    let _ = writeln!(
        out,
        "{} definitions to read, in the order below. Marks: {}.\n",
        review.reading.len(),
        Mark::legend(", ")
    );

    for (step, place) in review.reading.iter().zip(1..) {
        let Some(definition) = review.definitions.get(&step.definition) else {
            continue;
        };
        let mark = definition.mark.glyph();
        let latest = definition.sides.latest();
        let _ = writeln!(
            out,
            "## {place}. {mark} {} `{}`\n",
            names.get(&step.definition).map_or("?", String::as_str),
            latest.file
        );

        let broken_by: Vec<String> = walk::broken_by(review, step.definition)
            .iter()
            .map(named)
            .collect();
        let depends_on: Vec<String> = walk::depends_on(review, step.definition)
            .iter()
            .filter(|dependency| !walk::broken_by(review, step.definition).contains(dependency))
            .map(named)
            .collect();
        if !broken_by.is_empty() {
            let _ = writeln!(
                out,
                "Depends on contracts that changed: {}.\n",
                broken_by.join(", ")
            );
        }
        if !depends_on.is_empty() {
            let _ = writeln!(out, "Depends on: {}.\n", depends_on.join(", "));
        }
        if !step.on_faith.is_empty() {
            let on_faith: Vec<String> = step.on_faith.iter().map(named).collect();
            let _ = writeln!(out, "Taken on faith (a cycle): {}.\n", on_faith.join(", "));
        }

        let before = walk::stitched(definition.sides.before());
        let after = walk::stitched(definition.sides.after());
        let body = match (before.as_deref(), after.as_deref()) {
            (None, None) => String::new(),
            (Some(before), Some(after)) if before == after => diff::render_unchanged(after, &plain),
            (Some(before), Some(after)) => diff::render(before, after, &plain),
            (None, Some(after)) => diff::render_whole(after, '+', &plain),
            (Some(before), None) => diff::render_whole(before, '-', &plain),
        };
        if !body.is_empty() {
            let _ = writeln!(out, "```diff");
            // The terminal renderer indents every line by two; a fence wants none.
            for line in body.lines() {
                let _ = writeln!(out, "{}", line.strip_prefix("  ").unwrap_or(line));
            }
            let _ = writeln!(out, "```\n");
        }
    }

    if !review.warnings.is_empty() {
        let _ = writeln!(out, "## Warnings\n");
        for warning in &review.warnings {
            let _ = writeln!(out, "- {}", warning.message);
        }
    }
    out
}
