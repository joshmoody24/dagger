//! Working out which group each definition is in, from marker files.
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
                .or_insert_with(|| group_of(&config.markers, dir, file))
                .clone();
            (definition.identity, path)
        })
        .collect();

    Grouping {
        name: config.name.clone(),
        of,
        // Settled once the edges are known, which is after this.
        bands: Default::default(),
    }
}

/// The directory of the nearest marker at or above the file, as the group's path.
///
/// Where nothing says otherwise, a file's own directory is its group. Plenty of code isn't
/// packaged at all — a repository of C with no manifest anywhere in it — and left ungrouped
/// every file in it would sit in one bin, which tells a reader nothing about what's near
/// what. A directory is a weaker claim than a package, but it's the one people make when
/// they put files beside each other.
///
/// The odds and ends at the top of a repository fall out of the same rule: they share the
/// directory above them, which is nothing, so they're read together. They have nothing to
/// do with each other, and that's the reason to see them in one sitting rather than to
/// keep coming back to them between packages.
fn group_of(markers: &[String], dir: &Path, file: &str) -> GroupPath {
    let mut at = Path::new(file).parent();

    while let Some(here) = at {
        if markers
            .iter()
            .any(|marker| dir.join(here).join(marker).exists())
        {
            return named(here);
        }
        at = here.parent();
    }

    Path::new(file).parent().map(named).unwrap_or_default()
}

fn named(dir: &Path) -> GroupPath {
    dir.components()
        .map(|part| part.as_os_str().to_string_lossy().into_owned())
        .collect()
}
