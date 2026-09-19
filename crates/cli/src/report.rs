use dagger_core::change::Change;
use dagger_core::model::{Definition, Identity, Part};
use dagger_core::order::Ordering;
use dagger_core::review::Review;
use dagger_protocol::Note;
use std::collections::BTreeMap;
use std::io::Write;

/// A one-character shorthand for what happened, borrowed from the mock: additions and
/// removals stand out, and a contract change is louder than a body change.
fn glyph(change: &Change) -> char {
    match change {
        Change::Added => '+',
        Change::Removed => '-',
        Change::Kept(edits) if edits.contract => '!',
        Change::Kept(edits) if edits.changed(Part::Body) => '~',
        Change::Kept(edits) if edits.changed(Part::Docs) => '"',
        Change::Kept(_) => '.',
    }
}

fn name(definition: &Definition) -> String {
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
        "at most {} to hold in mind, {} taken on faith, {} jumps between files\n",
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
            format!(" (leans on {} still to come)", step.on_faith.len())
        };
        let _ = writeln!(
            out,
            "{place:>3}. {mark} {:<44} {}{affected}{faith}",
            name(definition),
            definition.sides.latest().file
        );
    }

    if !review.diagnostics.is_empty() || !notes.is_empty() {
        let _ = writeln!(
            out,
            "\n{} things worth knowing:",
            review.diagnostics.len() + notes.len()
        );
        for note in notes {
            let about = match &note.file {
                Some(file) => format!("{file}: "),
                None => String::new(),
            };
            let _ = writeln!(out, "  {about}{}", note.message);
        }
        for diagnostic in &review.diagnostics {
            let _ = writeln!(out, "  {diagnostic:?}");
        }
    }
}
