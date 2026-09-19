//! Drives the adapters, hands the results to the core, and prints what came back.
//!
//! Everything that touches a disk or starts a process lives on this side. The core is
//! given two piles of facts and nothing else.

mod adapter;
mod assign;
mod config;
mod fallback;
mod report;

use anyhow::{Result, bail};
use config::Config;
use dagger_core::matching::{Extraction, match_snapshots};
use dagger_core::review::review;
use std::path::Path;

struct Args {
    before: String,
    after: String,
    json: bool,
}

fn parse_args() -> Result<Args> {
    let mut positional = Vec::new();
    let mut json = false;

    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--json" => json = true,
            "-h" | "--help" => {
                println!("dagger [--json] [<before> <after>]");
                std::process::exit(0);
            }
            flag if flag.starts_with('-') => bail!("don't know the flag {flag}"),
            value => positional.push(value.to_string()),
        }
    }

    let (before, after) = match positional.as_slice() {
        [] => ("HEAD~1".to_string(), "HEAD".to_string()),
        [before, after] => (before.clone(), after.clone()),
        _ => bail!("expected two revisions, or none at all"),
    };

    Ok(Args {
        before,
        after,
        json,
    })
}

fn main() -> Result<()> {
    let args = parse_args()?;
    let repo = std::env::current_dir()?;
    let config = Config::read(&repo)?;

    let before = lay_out(&repo, &config, &args.before)?;
    let after = lay_out(&repo, &config, &args.after)?;

    let result = compare(&repo, &config, &before.dir, &after.dir, args.json);

    clean_up(&before);
    clean_up(&after);
    result
}

fn compare(repo: &Path, config: &Config, before: &Path, after: &Path, json: bool) -> Result<()> {
    let matched = match_snapshots(read(repo, config, before)?, read(repo, config, after)?);
    let review = review(&matched.definitions, &matched.references);

    if json {
        println!("{}", serde_json::to_string_pretty(&review)?);
    } else {
        report::print(&review, &matched.definitions);
    }
    Ok(())
}

/// Every extractor's answer for one snapshot, plus the leftovers, merged into the one
/// pile of facts the core expects.
fn read(repo: &Path, config: &Config, dir: &Path) -> Result<Extraction> {
    let assignment = assign::assign(config, dir)?;
    let mut merged = fallback::extract(dir, &assignment.fallback);

    for (extractor, files) in config.extractors.iter().zip(&assignment.extractors) {
        if files.is_empty() {
            continue;
        }
        let mut extracted = adapter::extract(repo, extractor, dir, files)?;
        merged.occurrences.append(&mut extracted.occurrences);
        merged.mentions.append(&mut extracted.mentions);
    }

    Ok(merged)
}

/// A revision named on the command line only means something if a snapshot adapter is
/// configured. Without one, the two arguments are just directories.
fn lay_out(repo: &Path, config: &Config, rev: &str) -> Result<adapter::Snapshot> {
    match &config.snapshots {
        Some(snapshots) => adapter::materialize(repo, snapshots, rev),
        None => {
            let dir = repo.join(rev);
            if !dir.is_dir() {
                bail!(
                    "no snapshot adapter is configured in {}, so {rev} has to be a directory",
                    config::FILE
                );
            }
            Ok(adapter::Snapshot {
                dir,
                temporary: false,
            })
        }
    }
}

fn clean_up(snapshot: &adapter::Snapshot) {
    if snapshot.temporary {
        let _ = std::fs::remove_dir_all(&snapshot.dir);
    }
}
