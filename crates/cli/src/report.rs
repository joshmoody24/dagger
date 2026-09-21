use dagger_core::change::Change;
use dagger_core::model::{Definition, Identity, Part};
use dagger_core::order::Ordering;
use dagger_core::review::Review;
use dagger_protocol::Note;
use std::collections::BTreeMap;
use std::io::Write;

/// A one-character shorthand for what happened, borrowed from the mock: additions and
/// removals stand out, and a contract change is louder than a body change.
pub fn glyph(change: &Change) -> char {
    match change {
        Change::Added => '+',
        Change::Removed => '-',
        Change::Kept(edits) if edits.contract => '!',
        // The signature was rewritten without callers noticing: reformatted, or a type
        // spelled a different way that means the same thing.
        Change::Kept(edits) if edits.changed(Part::Type) => '~',
        Change::Kept(edits) if edits.changed(Part::Body) => '~',
        Change::Kept(edits) if edits.changed(Part::Docs) => '"',
        Change::Kept(_) => '.',
    }
}

pub fn name(definition: &Definition) -> String {
    let occurrence = definition.sides.latest();
    let mut path = occurrence.locator.scope.clone();
    path.push(occurrence.locator.name.clone());
    path.join("::")
}

/// Writing rather than printing, because a reader quitting out of a pager closes the
/// pipe, and that shouldn't look like a crash.
pub fn print(review: &Review, ordering: &Ordering, definitions: &[Definition], notes: &[Note]) {
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    let by_identity: BTreeMap<Identity, &Definition> = definitions
        .iter()
        .map(|definition| (definition.identity, definition))
        .collect();

    // A definition can be both changed and downstream of someone else's change, so
    // these two counts overlap and don't add up to the total on purpose.
    let changed = review
        .members
        .iter()
        .filter(|identity| {
            review
                .changes
                .get(identity)
                .is_some_and(|change| change.worth_reading())
        })
        .count();

    let _ = writeln!(
        out,
        "{} definitions, {} to read ({} changed, {} affected)",
        definitions.len(),
        review.members.len(),
        changed,
        review.affected.len()
    );
    let _ = writeln!(
        out,
        "+ new   - gone   ! callers affected   ~ body   \" docs   . untouched\n\
         at most {} held in mind at once, {} read early, {} jumps between files\n",
        ordering.cost.peak_open, ordering.cost.taken_on_faith, ordering.cost.jumps
    );

    for (step, place) in ordering.steps.iter().zip(1..) {
        let identity = &step.definition;
        let Some(definition) = by_identity.get(identity) else {
            continue;
        };
        let mark = match review.changes.get(identity) {
            Some(change) if change.worth_reading() => glyph(change),
            _ => '=',
        };
        let affected = if review.affected.contains(identity) {
            " (affected)"
        } else {
            ""
        };
        let faith = if step.on_faith.is_empty() {
            String::new()
        } else {
            format!(" ({} not read yet)", step.on_faith.len())
        };
        let _ = writeln!(
            out,
            "{place:>3}. {mark} {:<44} {}{affected}{faith}",
            name(definition),
            definition.sides.latest().file
        );
    }

    // Whatever might be hiding a change is said first, and said as the worse news it is.
    // Told all together, the one that matters is buried among the ones that don't.
    let (hiding, weaker): (Vec<_>, Vec<_>) = review
        .diagnostics
        .iter()
        .partition(|diagnostic| diagnostic.hides());

    if !hiding.is_empty() || !notes.is_empty() {
        let _ = writeln!(
            out,
            "\n{} things this review might not be showing:",
            hiding.len() + notes.len()
        );
        for note in notes {
            let about = match &note.file {
                Some(file) => format!("{file}: "),
                None => String::new(),
            };
            let _ = writeln!(out, "  {about}{}", note.message);
        }
        for diagnostic in hiding {
            let _ = writeln!(out, "  {diagnostic:?}");
        }
    }

    if !weaker.is_empty() {
        let _ = writeln!(out, "\n{} worked out a weaker way:", weaker.len());
        for diagnostic in weaker {
            let _ = writeln!(out, "  {diagnostic:?}");
        }
    }
}
