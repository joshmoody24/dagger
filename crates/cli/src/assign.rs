//! Decides which extractor gets each file before any adapter starts, so adapters never
//! do glob matching and a surprising assignment can be printed.

use anyhow::{Context, Result};
use glob::Pattern;
use std::path::{Path, PathBuf};

#[derive(Debug, Default)]
pub struct Assignment {
    /// Files for each configured extractor, in the order they were configured.
    pub extractors: Vec<Vec<String>>,
    /// Files nobody claimed, which get read whole.
    pub fallback: Vec<String>,
    pub ignored: usize,
}

/// `claims` is one set of globs per extractor. `listed` is the snapshot adapter's file
/// list, preferred over walking since only it knows a build directory from a source one.
pub fn assign(
    ignore: &[String],
    claims: &[Vec<String>],
    dir: &Path,
    listed: Option<&[String]>,
) -> Result<Assignment> {
    let ignore = patterns(ignore)?;
    let claims: Vec<Vec<Pattern>> = claims
        .iter()
        .map(|globs| patterns(globs))
        .collect::<Result<_>>()?;

    let mut assignment = Assignment {
        extractors: vec![Vec::new(); claims.len()],
        ..Assignment::default()
    };

    let files = match listed {
        Some(listed) => listed.to_vec(),
        None => walked(dir)?,
    };

    for file in files {
        if ignore.iter().any(|pattern| pattern.matches(&file)) {
            assignment.ignored += 1;
            continue;
        }
        // Last claim wins, so a broad rule can be narrowed by a later one.
        match claims
            .iter()
            .rposition(|patterns| patterns.iter().any(|pattern| pattern.matches(&file)))
        {
            Some(extractor) => assignment.extractors[extractor].push(file),
            None => assignment.fallback.push(file),
        }
    }

    Ok(assignment)
}

fn patterns(globs: &[String]) -> Result<Vec<Pattern>> {
    globs
        .iter()
        .map(|glob| Pattern::new(glob).with_context(|| format!("{glob} isn't a valid pattern")))
        .collect()
}

pub fn walk_all(dir: &Path) -> Result<Vec<String>> {
    walked(dir)
}

/// An ignored file is out of the review entirely, so neither the extractors nor the
/// completeness check should see it.
pub fn not_ignored(ignore: &[String], files: Vec<String>) -> Result<Vec<String>> {
    let ignore = patterns(ignore)?;
    Ok(files
        .into_iter()
        .filter(|file| !ignore.iter().any(|pattern| pattern.matches(file)))
        .collect())
}

fn walked(dir: &Path) -> Result<Vec<String>> {
    let mut found = Vec::new();
    walk(dir, dir, &mut found)?;
    found.sort();
    Ok(found)
}

fn walk(root: &Path, dir: &Path, found: &mut Vec<String>) -> Result<()> {
    for entry in std::fs::read_dir(dir).with_context(|| format!("couldn't read {dir:?}"))? {
        let path: PathBuf = entry?.path();
        if path.is_dir() {
            walk(root, &path, found)?;
        } else {
            found.push(path.strip_prefix(root)?.to_string_lossy().into_owned());
        }
    }
    Ok(())
}
