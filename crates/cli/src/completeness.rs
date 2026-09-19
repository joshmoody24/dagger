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
            let lines = unaccounted(
                &read(before, file),
                &read(after, file),
                was_covered.get(file.as_str()).map(Vec::as_slice),
                is_covered.get(file.as_str()).map(Vec::as_slice),
            );

            (lines > 0).then(|| Diagnostic::Unattributed {
                file: file.clone(),
                lines,
            })
        })
        .collect()
}

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
    for (part, text) in &occurrence.parts {
        let file = occurrence
            .file_of(*part)
            .unwrap_or(occurrence.file.as_str());
        coverage
            .entry(file)
            .or_default()
            .push(text.span.start as usize..text.span.end as usize);
    }
}

fn read(dir: &Path, file: &str) -> String {
    std::fs::read_to_string(dir.join(file)).unwrap_or_default()
}

/// How many differing lines no definition speaks for. A line that was deleted is checked
/// against the older side, an added one against the newer.
fn unaccounted(
    before: &str,
    after: &str,
    was_covered: Option<&[Range<usize>]>,
    is_covered: Option<&[Range<usize>]>,
) -> usize {
    let (was, is) = (offsets(before), offsets(after));
    let diff = TextDiff::from_lines(before, after);
    let mut missed = 0;

    let mut old_line = 0usize;
    let mut new_line = 0usize;
    for change in diff.iter_all_changes() {
        match change.tag() {
            ChangeTag::Equal => {
                old_line += 1;
                new_line += 1;
            }
            ChangeTag::Delete => {
                if !inside(was.get(old_line), was_covered) {
                    missed += 1;
                }
                old_line += 1;
            }
            ChangeTag::Insert => {
                if !inside(is.get(new_line), is_covered) {
                    missed += 1;
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

/// Blank lines and the like are let through: a definition can't be expected to claim the
/// gap between itself and the next one.
fn inside(start: Option<&usize>, covered: Option<&[Range<usize>]>) -> bool {
    let Some(&start) = start else {
        return true;
    };
    covered
        .unwrap_or_default()
        .iter()
        .any(|range| range.contains(&start) || range.end == start)
}
