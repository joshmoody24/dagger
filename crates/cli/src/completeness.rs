//! Checking that nothing changed without being accounted for.
//!
//! The review is only as honest as the extractors: anything they walk past simply isn't
//! in it, and the output looks just as tidy either way. So every line that differs between
//! the snapshots is checked against the definitions covering that file, and whatever falls
//! outside all of them is reported.
//!
//! Imports are the usual answer. Nobody has written an extractor yet that calls an import
//! statement a definition, so a commit that only changes what a file pulls in can produce
//! a review with nothing in it.

use dagger_core::diagnostic::Diagnostic;
use dagger_core::model::{Definition, Occurrence};
use similar::{ChangeTag, TextDiff};
use std::collections::BTreeMap;
use std::ops::Range;
use std::path::Path;

pub fn check(
    before: &Path,
    after: &Path,
    changed: &[String],
    definitions: &[Definition],
) -> Vec<Diagnostic> {
    let (was_covered, is_covered) = covered(definitions);

    changed
        .iter()
        .filter_map(|file| {
            let missed = unaccounted(
                &read(before, file),
                &read(after, file),
                was_covered.get(file.as_str()).map(Vec::as_slice),
                is_covered.get(file.as_str()).map(Vec::as_slice),
            );

            (!missed.is_empty()).then(|| Diagnostic::Unattributed {
                file: file.clone(),
                lines: missed.len(),
                at: missed.into_iter().take(SHOWN).collect(),
            })
        })
        .collect()
}

/// Enough line numbers to go and look, not enough to drown the rest of the output.
const SHOWN: usize = 5;

/// Which stretches of each file some definition speaks for, on each side. A part that
/// named its own file is counted against that one, the way a C declaration in a header is.
type Coverage<'a> = BTreeMap<&'a str, Vec<Range<usize>>>;

fn covered(definitions: &[Definition]) -> (Coverage<'_>, Coverage<'_>) {
    let mut was = Coverage::new();
    let mut is = Coverage::new();

    for definition in definitions {
        if let Some(occurrence) = definition.sides.before() {
            note(&mut was, occurrence);
        }
        if let Some(occurrence) = definition.sides.after() {
            note(&mut is, occurrence);
        }
    }

    (was, is)
}

fn note<'a>(coverage: &mut Coverage<'a>, occurrence: &'a Occurrence) {
    for (file, piece) in occurrence.pieces() {
        coverage
            .entry(file)
            .or_default()
            .push(piece.span.start as usize..piece.span.end as usize);
    }
}

fn read(dir: &Path, file: &str) -> String {
    std::fs::read_to_string(dir.join(file)).unwrap_or_default()
}

/// Which differing lines no definition speaks for, numbered from one as an editor would.
/// A line that was deleted is checked against the older side, an added one against the
/// newer, and reported at whichever side it belongs to.
fn unaccounted(
    before: &str,
    after: &str,
    was_covered: Option<&[Range<usize>]>,
    is_covered: Option<&[Range<usize>]>,
) -> Vec<u32> {
    let (was, is) = (offsets(before), offsets(after));
    let diff = TextDiff::from_lines(before, after);
    let mut missed = Vec::new();

    let mut old_line = 0usize;
    let mut new_line = 0usize;
    for change in diff.iter_all_changes() {
        // A blank line belongs to nobody. Definitions sit apart from each other, and the
        // space between them isn't something a reader was going to look at.
        let blank = change.value().trim().is_empty();

        match change.tag() {
            ChangeTag::Equal => {
                old_line += 1;
                new_line += 1;
            }
            ChangeTag::Delete => {
                if !blank && !inside(line_at(&was, old_line), was_covered) {
                    missed.push(old_line as u32 + 1);
                }
                old_line += 1;
            }
            ChangeTag::Insert => {
                if !blank && !inside(line_at(&is, new_line), is_covered) {
                    missed.push(new_line as u32 + 1);
                }
                new_line += 1;
            }
        }
    }

    missed
}

/// Where each line starts, so a line number can be matched against the byte ranges
/// extractors report.
fn offsets(text: &str) -> Vec<usize> {
    let mut starts = vec![0];
    starts.extend(
        text.char_indices()
            .filter(|(_, character)| *character == '\n')
            .map(|(at, _)| at + 1),
    );
    starts
}

/// The stretch of bytes one line occupies.
fn line_at(starts: &[usize], line: usize) -> Option<Range<usize>> {
    let start = *starts.get(line)?;
    let end = starts.get(line + 1).copied().unwrap_or(usize::MAX);
    Some(start..end)
}

/// Whether any definition speaks for any of this line.
///
/// Overlap rather than containment, because a definition rarely starts where its line does.
/// `pub fn start()` begins after the `pub`, a doc comment begins after the indentation, and
/// asking whether the line's first byte sits inside the range says no to both.
fn inside(line: Option<Range<usize>>, covered: Option<&[Range<usize>]>) -> bool {
    let Some(line) = line else {
        return true;
    };
    covered
        .unwrap_or_default()
        .iter()
        .any(|range| range.start < line.end && line.start < range.end)
}
