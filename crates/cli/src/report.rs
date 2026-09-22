use dagger_core::change::Change;
use dagger_core::model::Part;
use dagger_core::review::{Definition, Impact, Review};
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
pub fn print(review: &Review) {
    let stdout = std::io::stdout();
    let mut out = stdout.lock();

    // A definition can be both changed and downstream of someone else's change, so
    // these two counts overlap and don't add up to the total on purpose.
    let read = review.reading.len();
    let changed = review
        .reading
        .iter()
        .filter_map(|step| review.definitions.get(&step.definition))
        .filter(|one| one.change.worth_reading())
        .count();
    let reached = review
        .definitions
        .values()
        .filter(|one| one.reached.is_some())
        .count();

    let _ = writeln!(
        out,
        "{} definitions, {read} to read ({changed} changed, {reached} affected)",
        review.definitions.len(),
    );
    let _ = writeln!(
        out,
        "+ new   - gone   ! callers affected   ~ body   \" docs   . untouched\n\
         at most {} held in mind at once, {} read early, {} jumps between files\n",
        review.cost.peak_open, review.cost.taken_on_faith, review.cost.jumps
    );

    for (step, place) in review.reading.iter().zip(1..) {
        let Some(definition) = review.definitions.get(&step.definition) else {
            continue;
        };
        let mark = match definition.change.worth_reading() {
            true => glyph(&definition.change),
            false => '=',
        };
        let affected = match definition.reached {
            Some(_) => " (affected)",
            None => "",
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
        .warnings
        .iter()
        .partition(|warning| warning.impact == Impact::Incomplete);

    if !hiding.is_empty() {
        let _ = writeln!(
            out,
            "\n{} things this review might not be showing:",
            hiding.len()
        );
        for warning in hiding {
            let _ = writeln!(out, "  {}", warning.message);
        }
    }

    if !weaker.is_empty() {
        let _ = writeln!(out, "\n{} worked out a weaker way:", weaker.len());
        for warning in weaker {
            let _ = writeln!(out, "  {}", warning.message);
        }
    }
}
