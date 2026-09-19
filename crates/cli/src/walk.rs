//! Stepping through a review one definition at a time.
//!
//! Deliberately thin. It exists to find out whether the reading order is any good, so
//! anything fancier belongs in a real viewer rather than here.
//!
//! What it shows that a plain diff can't: which of the things this leans on you've
//! already read, and which are still to come. That's the whole point of having
//! bothered to work out an order.

use crate::diff::{self, BOLD, DIM, Paint};
use crate::report;
use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use dagger_core::model::{Definition, Identity, Occurrence, PartText, Sides};
use dagger_core::order::{Ordering, Step};
use dagger_core::review::Review;
use std::collections::BTreeMap;
use std::io::Write;

enum Move {
    Next,
    Back,
    Quit,
}

pub fn walk(review: &Review, ordering: &Ordering, definitions: &[Definition]) -> Result<()> {
    let paint = Paint::new(true);
    let by_identity: BTreeMap<Identity, &Definition> = definitions
        .iter()
        .map(|definition| (definition.identity, definition))
        .collect();
    let names: BTreeMap<Identity, String> = definitions
        .iter()
        .map(|definition| (definition.identity, report::name(definition)))
        .collect();

    let mut place = 0usize;
    while place < ordering.steps.len() {
        let step = &ordering.steps[place];
        let Some(definition) = by_identity.get(&step.definition) else {
            place += 1;
            continue;
        };

        show(review, ordering, step, definition, place, &names, &paint);

        match wait()? {
            Move::Next => place += 1,
            Move::Back => place = place.saturating_sub(1),
            Move::Quit => return Ok(()),
        }
    }

    println!("\nreview complete");
    Ok(())
}

fn show(
    review: &Review,
    ordering: &Ordering,
    step: &Step,
    definition: &Definition,
    place: usize,
    names: &BTreeMap<Identity, String>,
    paint: &Paint,
) {
    let identity = step.definition;
    let name = names
        .get(&identity)
        .cloned()
        .unwrap_or_else(|| "?".to_string());
    let mark = review
        .changes
        .get(&identity)
        .map(report::glyph)
        .unwrap_or('=');

    println!(
        "\n{} {mark} {}  {}",
        paint.wrap(DIM, &format!("{}/{}", place + 1, ordering.steps.len())),
        paint.wrap(BOLD, &name),
        paint.wrap(DIM, &definition.sides.latest().file),
    );

    let unseen = |leaned: &Identity| step.on_faith.contains(leaned);
    let named = |leaned: &Identity| names.get(leaned).map(|name| (short(name), unseen(leaned)));

    let blamed = culprits(review, identity);
    let because: Vec<(String, bool)> = blamed.iter().filter_map(named).collect();
    if !because.is_empty() {
        println!(
            "  {} {}",
            paint.wrap(DIM, "because:"),
            paint.wrap(BOLD, &listed(&because))
        );
    }

    let rest: Vec<(String, bool)> = leaned_on(review, identity)
        .iter()
        .filter(|leaned| !blamed.contains(leaned))
        .filter_map(named)
        .collect();
    if !rest.is_empty() {
        println!(
            "  {} {}",
            paint.wrap(DIM, "uses:"),
            paint.wrap(BOLD, &listed(&rest))
        );
    }

    print_parts(&definition.sides, paint);
}

/// Just the name. The module path is already on the line above, and these are things
/// the reader saw minutes ago.
fn short(name: &str) -> String {
    name.rsplit("::").next().unwrap_or(name).to_string()
}

/// Long lists stop being read, so say how many rather than all of them. Anything the
/// reader hasn't got to yet is marked, since that's the bit they can't check.
fn listed(names: &[(String, bool)]) -> String {
    const SHOWN: usize = 6;
    let written: Vec<String> = names
        .iter()
        .take(SHOWN)
        .map(|(name, unseen)| {
            if *unseen {
                format!("{name} (not yet seen)")
            } else {
                name.clone()
            }
        })
        .collect();

    if names.len() <= SHOWN {
        return written.join(", ");
    }
    format!("{}, and {} more", written.join(", "), names.len() - SHOWN)
}

/// What this leans on that changed shape underneath it. The whole reason an unchanged
/// definition is worth a reader's time, so it shouldn't be left to them to work out.
fn culprits(review: &Review, identity: Identity) -> Vec<Identity> {
    leaned_on(review, identity)
        .into_iter()
        .filter(|leaned| {
            review
                .changes
                .get(leaned)
                .is_some_and(|change| change.breaks_callers())
        })
        .collect()
}

/// What a definition leans on, going by the review's own edges so a chain through
/// something the reader never sees still counts.
fn leaned_on(review: &Review, identity: Identity) -> Vec<Identity> {
    review
        .edges
        .iter()
        .filter(|edge| edge.from == identity)
        .map(|edge| edge.to)
        .collect()
}

/// The parts stitched back together in the order they appear in the file, which is
/// how they were written and how they read. The split into parts is for deciding what
/// breaks callers, not for showing people.
fn print_parts(sides: &Sides, paint: &Paint) {
    let before = stitched(sides.before());
    let after = stitched(sides.after());

    let body = match (before.as_deref(), after.as_deref()) {
        (None, None) => return,
        // Nothing of its own changed, so there's no diff to read. Show it as it stands,
        // which is what the reader has to judge against whatever moved underneath.
        (Some(before), Some(after)) if before == after => {
            println!();
            print!("{}", diff::render_unchanged(after, paint));
            return;
        }
        (Some(before), Some(after)) => diff::render(before, after, paint),
        (None, Some(after)) => diff::render_whole(after, '+', paint),
        (Some(before), None) => diff::render_whole(before, '-', paint),
    };

    println!();
    print!("{body}");
}

fn stitched(occurrence: Option<&Occurrence>) -> Option<String> {
    let occurrence = occurrence?;
    let mut parts: Vec<&PartText> = occurrence.parts.values().collect();
    parts.sort_by_key(|part| part.span.start);

    Some(
        parts
            .iter()
            .map(|part| part.text.trim_matches('\n'))
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join("\n"),
    )
}

/// Raw mode only while waiting, so everything printed keeps ordinary line endings.
fn wait() -> Result<Move> {
    print!(
        "\n{}",
        Paint::new(true).wrap(DIM, "j next   k back   q quit  ")
    );
    std::io::stdout().flush()?;

    enable_raw_mode()?;
    let moved = loop {
        if let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
        {
            match key.code {
                KeyCode::Char('q') | KeyCode::Esc => break Move::Quit,
                KeyCode::Char('k') | KeyCode::Up | KeyCode::Backspace => break Move::Back,
                KeyCode::Char('j') | KeyCode::Down | KeyCode::Char(' ') | KeyCode::Enter => {
                    break Move::Next;
                }
                _ => {}
            }
        }
    };
    disable_raw_mode()?;
    println!();

    Ok(moved)
}
