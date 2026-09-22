//! Reads files no extractor claimed, one definition per file. Coarse on purpose, so
//! nothing in a review can go unmentioned.

use dagger_core::matching::Extraction;
use dagger_core::model::{Locator, Occurrence, Part, Piece, Role, Span, segments};
use std::collections::BTreeMap;
use std::path::Path;

pub fn extract(dir: &Path, files: &[String]) -> Extraction {
    Extraction {
        occurrences: files
            .iter()
            .filter_map(|file| occurrence(dir, file))
            .collect(),
        mentions: Vec::new(),
    }
}

/// Binary files are skipped, since there's no sensible way to show one changing.
fn occurrence(dir: &Path, file: &str) -> Option<Occurrence> {
    let text = std::fs::read_to_string(dir.join(file)).ok()?;
    let path = Path::new(file);
    let name = path.file_name()?.to_string_lossy().into_owned();
    let scope = path.parent().map(segments).unwrap_or_default();

    let span = Span {
        start: 0,
        end: text.len() as u32,
    };
    Some(Occurrence {
        locator: Locator { scope, name },
        // A whole file has no structure to report.
        role: Role::Item,
        parent: None,
        kind: "file".to_string(),
        file: file.to_string(),
        parts: BTreeMap::from([(
            Part::Body,
            vec![Piece {
                text,
                span,
                line: 1,
                file: file.to_string(),
            }],
        )]),
        contract_from_compiler: None,
    })
}
