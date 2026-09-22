//! Reports differing lines that no definition covers. Anything an extractor skips
//! (imports, usually) would otherwise silently drop out of the review.

use dagger_core::diagnostic::Diagnostic;
use dagger_core::model::{Definition, Occurrence};
use dagger_core::prose::line_starts;
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

/// Byte ranges some definition covers, per file. A part in another file (a C declaration
/// in a header) counts against that file.
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

/// Differing lines no definition covers, numbered from one. Deleted lines are checked
/// against the before side, added lines against the after side.
fn unaccounted(
    before: &str,
    after: &str,
    was_covered: Option<&[Range<usize>]>,
    is_covered: Option<&[Range<usize>]>,
) -> Vec<u32> {
    let (was, is) = (line_starts(before), line_starts(after));
    let diff = TextDiff::from_lines(before, after);
    let mut missed = Vec::new();

    let mut old_line = 0usize;
    let mut new_line = 0usize;
    for change in diff.iter_all_changes() {
        // Blank lines between definitions aren't worth reporting.
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

/// The stretch of bytes one line occupies.
fn line_at(starts: &[usize], line: usize) -> Option<Range<usize>> {
    let start = *starts.get(line)?;
    let end = starts.get(line + 1).copied().unwrap_or(usize::MAX);
    Some(start..end)
}

/// Overlap rather than containment, since a definition rarely starts at its line's first
/// byte (`pub fn` starts after the `pub`, a doc comment after the indentation).
fn inside(line: Option<Range<usize>>, covered: Option<&[Range<usize>]>) -> bool {
    let Some(line) = line else {
        return true;
    };
    covered
        .unwrap_or_default()
        .iter()
        .any(|range| range.start < line.end && line.start < range.end)
}
