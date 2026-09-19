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
use dagger_core::model::{Definition, Identity, Occurrence, Part, Sides};
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

    println!("\nthat's all of it.");
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

    if review.affected.contains(&identity) {
        let changed_too = review
            .changes
            .get(&identity)
            .is_some_and(|change| change.worth_reading());
        let line = if changed_too {
            "changed, and something it leans on changed under it too"
        } else {
            "didn't change on its own; something it leans on did"
        };
        println!("  {}", paint.wrap(DIM, line));
    }

    let read_already: Vec<String> = leaned_on(review, identity)
        .into_iter()
        .filter(|leaned| !step.on_faith.contains(leaned))
        .filter_map(|leaned| names.get(&leaned).map(|name| short(name)))
        .collect();
    if !read_already.is_empty() {
        println!(
            "  {}",
            paint.wrap(DIM, &format!("builds on: {}", listed(&read_already)))
        );
    }

    let to_come: Vec<String> = step
        .on_faith
        .iter()
        .filter_map(|leaned| names.get(leaned).map(|name| short(name)))
        .collect();
    if !to_come.is_empty() {
        println!(
            "  {}",
            paint.wrap(DIM, &format!("take on faith for now: {}", listed(&to_come)))
        );
    }

    print_parts(&definition.sides, paint);
}

/// Just the name. The module path is already on the line above, and these are things
/// the reader saw minutes ago.
fn short(name: &str) -> String {
    name.rsplit("::").next().unwrap_or(name).to_string()
}

/// Long lists stop being read, so say how many rather than all of them.
fn listed(names: &[String]) -> String {
    const SHOWN: usize = 6;
    if names.len() <= SHOWN {
        return names.join(", ");
    }
    format!(
        "{}, and {} more",
        names[..SHOWN].join(", "),
        names.len() - SHOWN
    )
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

fn print_parts(sides: &Sides, paint: &Paint) {
    for part in [Part::Docs, Part::Type, Part::Body] {
        let before = text_of(sides.before(), part);
        let after = text_of(sides.after(), part);

        let body = match (before, after) {
            (None, None) => continue,
            (Some(before), Some(after)) if before == after => continue,
            (Some(before), Some(after)) => diff::render(before, after, paint),
            (None, Some(after)) => diff::render_whole(after, '+', paint),
            (Some(before), None) => diff::render_whole(before, '-', paint),
        };

        println!(
            "\n  {}",
            paint.wrap(DIM, &format!("{part:?}").to_lowercase())
        );
        print!("{body}");
    }
}

fn text_of(occurrence: Option<&Occurrence>, part: Part) -> Option<&str> {
    occurrence?.parts.get(&part).map(|text| text.text.as_str())
}

/// Raw mode only while waiting, so everything printed keeps ordinary line endings.
fn wait() -> Result<Move> {
    print!(
        "\n  {}",
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
