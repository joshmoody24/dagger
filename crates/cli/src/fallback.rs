//! Reads whatever no extractor claimed, one definition per file.
//!
//! Coarse on purpose. It exists so nothing in a review can go unmentioned, whether
//! it's a lockfile, a pile of YAML, or a language nobody has written an adapter for.

use dagger_core::matching::Extraction;
use dagger_core::model::{Locator, Occurrence, Part, Piece, Role, Span};
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

/// Binary files are skipped. There's nothing a reader could do with one, and no
/// sensible way to show it changing.
fn occurrence(dir: &Path, file: &str) -> Option<Occurrence> {
    let text = std::fs::read_to_string(dir.join(file)).ok()?;
    let path = Path::new(file);
    let name = path.file_name()?.to_string_lossy().into_owned();
    let scope = path
        .parent()
        .map(|parent| {
            parent
                .components()
                .map(|part| part.as_os_str().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();

    let span = Span {
        start: 0,
        end: text.len() as u32,
    };
    Some(Occurrence {
        locator: Locator { scope, name },
        // A whole file read as one definition holds nothing and sits inside nothing: there
        // is no structure here to report, which is what falling back means.
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
        contract: None,
    })
}
