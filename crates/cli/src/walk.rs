//! Steps through a review one definition at a time, showing which dependencies were
//! already read and which are still to come. Deliberately thin: it exists to check
//! whether the reading order is any good, so anything fancier belongs in a real viewer.

use crate::diff::{self, BOLD, DIM, Paint};
use crate::report;
use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use dagger_core::model::{Identity, Occurrence, Sides};
use dagger_core::order::Step;
use dagger_core::review::{Definition, Review};
use std::collections::BTreeMap;
use std::io::Write;

enum Move {
    Next,
    Back,
    Quit,
}

pub fn walk(review: &Review) -> Result<()> {
    let paint = Paint::new(true);
    let names: BTreeMap<Identity, String> = review
        .definitions
        .iter()
        .map(|(identity, definition)| (*identity, report::name(definition)))
        .collect();

    let mut place = 0usize;
    while place < review.reading.len() {
        let step = &review.reading[place];
        let Some(definition) = review.definitions.get(&step.definition) else {
            place += 1;
            continue;
        };

        show(review, step, definition, place, &names, &paint);

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
        .definitions
        .get(&identity)
        .map_or('=', |one| one.mark.glyph());

    println!(
        "\n{} {mark} {}  {}",
        paint.wrap(DIM, &format!("{}/{}", place + 1, review.reading.len())),
        paint.wrap(BOLD, &name),
        paint.wrap(DIM, &definition.sides.latest().file),
    );

    let unread = |dependency: &Identity| step.on_faith.contains(dependency);
    let named = |dependency: &Identity| {
        names
            .get(dependency)
            .map(|name| (short(name), unread(dependency)))
    };

    let blamed = broken_by(review, identity);
    let broken_by: Vec<(String, bool)> = blamed.iter().filter_map(named).collect();
    if !broken_by.is_empty() {
        println!(
            "  {} {}",
            paint.wrap(DIM, "broken by:"),
            paint.wrap(BOLD, &listed(&broken_by))
        );
    }

    let rest: Vec<(String, bool)> = depends_on(review, identity)
        .iter()
        .filter(|dependency| !blamed.contains(dependency))
        .filter_map(named)
        .collect();
    if !rest.is_empty() {
        println!(
            "  {} {}",
            paint.wrap(DIM, "depends on:"),
            paint.wrap(BOLD, &listed(&rest))
        );
    }

    print_parts(&definition.sides, paint);
}

/// Just the name, since the module path is already on the line above.
pub fn short(name: &str) -> String {
    name.rsplit("::").next().unwrap_or(name).to_string()
}

/// Long lists stop being read, so they're capped. Unread entries are marked since those
/// are the ones the reader can't check.
fn listed(names: &[(String, bool)]) -> String {
    const SHOWN: usize = 6;
    let written: Vec<String> = names
        .iter()
        .take(SHOWN)
        .map(|(name, unread)| {
            if *unread {
                format!("{name} (not yet read)")
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

/// Dependencies that changed shape underneath this one, which is the reason an unchanged
/// definition is worth reading at all.
pub fn broken_by(review: &Review, identity: Identity) -> Vec<Identity> {
    depends_on(review, identity)
        .into_iter()
        .filter(|dependency| {
            review
                .definitions
                .get(dependency)
                .is_some_and(|one| one.change.breaks_callers())
        })
        .collect()
}

/// Uses the review's edges so a chain through something the reader never sees still counts.
pub fn depends_on(review: &Review, identity: Identity) -> Vec<Identity> {
    review
        .edges
        .iter()
        .filter(|edge| edge.from == identity)
        .map(|edge| edge.to)
        .collect()
}

fn print_parts(sides: &Sides, paint: &Paint) {
    let before = stitched(sides.before());
    let after = stitched(sides.after());

    let body = match (before.as_deref(), after.as_deref()) {
        (None, None) => return,
        // Unchanged, so show it as it stands to judge against whatever moved underneath.
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

/// Pieces in file order. The split into parts is for deciding what breaks callers, not
/// for showing people.
pub fn stitched(occurrence: Option<&Occurrence>) -> Option<String> {
    let occurrence = occurrence?;

    Some(
        occurrence
            .pieces()
            .iter()
            .map(|(_, piece)| piece.text.trim_matches('\n'))
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
