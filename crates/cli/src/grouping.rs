//! Works out each definition's group from marker files: the nearest directory above it
//! holding a `BUILD.bazel`, `Cargo.toml`, `package.json`, and so on. Anything a marker
//! can't express would need an adapter; nothing needs one yet.

use crate::config::GroupConfig;
use dagger_core::group::{Grouping, Path as GroupPath};
use dagger_core::model::{Definition, segments};
use std::collections::BTreeMap;
use std::path::Path;

pub fn of(config: &GroupConfig, dir: &Path, definitions: &[Definition]) -> Grouping {
    let mut known: BTreeMap<String, GroupPath> = BTreeMap::new();

    let of = definitions
        .iter()
        .map(|definition| {
            let file = &definition.sides.latest().file;
            let path = known
                .entry(file.clone())
                .or_insert_with(|| group_of(&config.markers, dir, file))
                .clone();
            (definition.identity, path)
        })
        .collect();

    Grouping {
        name: config.name.clone(),
        of,
    }
}

/// The nearest marker's directory, or the file's own directory when there is none, so
/// unpackaged code (plain C, say) isn't all lumped into one bin. Top-level files share
/// the empty directory and so are read together.
fn group_of(markers: &[String], dir: &Path, file: &str) -> GroupPath {
    let mut at = Path::new(file).parent();

    while let Some(here) = at {
        if markers
            .iter()
            .any(|marker| dir.join(here).join(marker).exists())
        {
            return segments(here);
        }
        at = here.parent();
    }

    Path::new(file).parent().map(segments).unwrap_or_default()
}
