//! Deciding who speaks for each file, before any adapter is started.
//!
//! Keeping this on our side means adapter authors never write glob matching, and a
//! surprising assignment can be printed rather than guessed at.

use crate::config::Config;
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

pub fn assign(config: &Config, dir: &Path) -> Result<Assignment> {
    let ignore = patterns(&config.review.ignore)?;
    let claims: Vec<Vec<Pattern>> = config
        .extractors
        .iter()
        .map(|extractor| patterns(&extractor.include))
        .collect::<Result<_>>()?;

    let mut assignment = Assignment {
        extractors: vec![Vec::new(); config.extractors.len()],
        ..Assignment::default()
    };

    for file in files(dir)? {
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

fn files(dir: &Path) -> Result<Vec<String>> {
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
