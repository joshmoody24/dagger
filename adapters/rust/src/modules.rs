//! Works out a file's Rust module path.
//!
//! The crate name comes from the nearest Cargo.toml, and `src`, `lib.rs`, `main.rs` and
//! `mod.rs` name no module, so a locator survives the crate moving directories.

use dagger_core::model::segments;
use dagger_protocol::Note;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Default)]
pub struct Modules {
    crates: BTreeMap<PathBuf, Option<String>>,
    pub notes: Vec<Note>,
}

impl Modules {
    pub fn path_of(&mut self, root: &Path, file: &str) -> Vec<String> {
        let file = Path::new(file);
        let Some((manifest, name)) = self.enclosing_crate(root, file) else {
            self.notes.push(Note {
                message: "no Cargo.toml above this, so its path is standing in for a \
                          module path"
                    .to_string(),
                file: Some(file.to_string_lossy().into_owned()),
            });
            return fallback(file);
        };

        let mut path = vec![name];
        path.extend(inside_crate(&manifest, file));
        path
    }

    /// The nearest Cargo.toml at or above the file, and the package it declares.
    fn enclosing_crate(&mut self, root: &Path, file: &Path) -> Option<(PathBuf, String)> {
        let mut dir = file.parent();
        while let Some(current) = dir {
            let manifest = current.join("Cargo.toml");
            let name = self
                .crates
                .entry(manifest.clone())
                .or_insert_with(|| package_name(&root.join(&manifest)))
                .clone();
            if let Some(name) = name {
                return Some((current.to_path_buf(), name));
            }
            dir = current.parent();
        }
        None
    }
}

/// The module segments between a crate's `src` and the file.
fn inside_crate(crate_dir: &Path, file: &Path) -> Vec<String> {
    let Ok(relative) = file.strip_prefix(crate_dir) else {
        return Vec::new();
    };

    segments(&relative.with_extension(""))
        .into_iter()
        .filter(|part| !matches!(part.as_str(), "src" | "lib" | "main" | "mod"))
        .collect()
}

/// No Cargo.toml anywhere above, so the path is all we have to go on.
fn fallback(file: &Path) -> Vec<String> {
    segments(&file.with_extension(""))
}

#[derive(Deserialize)]
struct Manifest {
    package: Option<Package>,
}

#[derive(Deserialize)]
struct Package {
    name: String,
}

/// A workspace root has no `[package]`, so it reports nothing and the walk upwards
/// carries on past it.
fn package_name(manifest: &Path) -> Option<String> {
    let text = std::fs::read_to_string(manifest).ok()?;
    let parsed: Manifest = toml::from_str(&text).ok()?;
    Some(parsed.package?.name.replace('-', "_"))
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_manifest_gives_up_its_package_name() {
        let dir = std::env::temp_dir().join("dagger-modules-test");
        std::fs::create_dir_all(&dir).unwrap();
        let manifest = dir.join("Cargo.toml");
        std::fs::write(&manifest, "[package]\nname = \"dagger-core\"\n").unwrap();

        assert_eq!(
            super::package_name(&manifest),
            Some("dagger_core".to_string())
        );
    }
}
