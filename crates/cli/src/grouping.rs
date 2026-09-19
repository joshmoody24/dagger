//! Working out which group each definition is in, from a marker file.
//!
//! Almost every real grouping turns out to be "the nearest directory above this one holding
//! a particular file". A bazel package is the nearest `BUILD.bazel`, a crate the nearest
//! `Cargo.toml`, a workspace package the nearest `package.json`. One line of configuration
//! covers all of them, and no subprocess has to be started to answer it.
//!
//! What a marker can't express — who owns a directory, what a build graph says — is what an
//! adapter would be for. Nothing needs one yet.

use crate::config::GroupingConfig;
use dagger_core::group::{Grouping, Path as GroupPath};
use dagger_core::model::Definition;
use std::collections::BTreeMap;
use std::path::Path;

pub fn of(config: &GroupingConfig, dir: &Path, definitions: &[Definition]) -> Grouping {
    let mut known: BTreeMap<String, GroupPath> = BTreeMap::new();

    let of = definitions
        .iter()
        .map(|definition| {
            let file = &definition.sides.latest().file;
            let path = known
                .entry(file.clone())
                .or_insert_with(|| group_of(&config.marker, dir, file))
                .clone();
            (definition.identity, path)
        })
        .collect();

    Grouping {
        name: config.name.clone(),
        of,
    }
}

/// The directory of the nearest marker at or above the file, as the group's path. A file
/// with no marker above it belongs to no group, which is how the outermost odds and ends end
/// up outside every box rather than in a pretend one.
fn group_of(marker: &str, dir: &Path, file: &str) -> GroupPath {
    let mut at = Path::new(file).parent();

    while let Some(here) = at {
        if dir.join(here).join(marker).exists() {
            return here
                .components()
                .map(|part| part.as_os_str().to_string_lossy().into_owned())
                .collect();
        }
        at = here.parent();
    }

    Vec::new()
}
