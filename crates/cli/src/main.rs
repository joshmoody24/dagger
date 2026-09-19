//! Drives the adapters, hands the results to the core, and prints what came back.
//!
//! Everything that touches a disk or starts a process lives on this side. The core is
//! given two piles of facts and nothing else.

mod adapter;
mod assign;
mod config;
mod diff;
mod fallback;
mod report;
mod walk;

use anyhow::{Result, bail};
use config::Config;
use dagger_core::matching::{Extraction, match_snapshots};
use dagger_core::order::order;
use dagger_core::review::review;
use dagger_protocol::Note;
use std::io::{IsTerminal, Write};
use std::path::Path;

struct Args {
    before: String,
    after: String,
    json: bool,
    explain: bool,
    list: bool,
}

fn parse_args() -> Result<Args> {
    let mut positional = Vec::new();
    let mut json = false;
    let mut explain = false;
    let mut list = false;

    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--json" => json = true,
            "--explain" => explain = true,
            "--list" => list = true,
            "-h" | "--help" => {
                println!("dagger [--json] [--explain] [--list] [<before> <after>]");
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
        explain,
        list,
    })
}

fn main() -> Result<()> {
    let args = parse_args()?;
    let repo = std::env::current_dir()?;
    let config = Config::read(&repo)?;

    let claims = claims(&repo, &config)?;

    let before = lay_out(&repo, &config, &args.before)?;
    let after = lay_out(&repo, &config, &args.after)?;

    let result = if args.explain {
        explain(&config, &claims, &after)
    } else {
        compare(&repo, &config, &claims, &before, &after, &args)
    };

    clean_up(&before);
    clean_up(&after);
    result
}

/// What each extractor will be given: what the repo asked for, or failing that, what
/// the adapter says it reads.
fn claims(repo: &Path, config: &Config) -> Result<Vec<Vec<String>>> {
    config
        .extractors
        .iter()
        .map(|extractor| {
            if extractor.include.is_empty() {
                adapter::describe(repo, extractor)
            } else {
                Ok(extractor.include.clone())
            }
        })
        .collect()
}

/// Who ended up with what, so a surprising assignment can be looked at instead of
/// guessed at.
fn explain(config: &Config, claims: &[Vec<String>], snapshot: &adapter::Snapshot) -> Result<()> {
    let assignment = assign::assign(
        &config.review.ignore,
        claims,
        &snapshot.dir,
        snapshot.files.as_deref(),
    )?;

    for ((extractor, claim), files) in config
        .extractors
        .iter()
        .zip(claims)
        .zip(&assignment.extractors)
    {
        let source = if extractor.include.is_empty() {
            "declared"
        } else {
            "configured"
        };
        println!(
            "{:<34} {:>5} files  ({source} {})",
            extractor.adapter,
            files.len(),
            claim.join(" ")
        );
    }

    println!("{:<34} {:>5} files", "fallback", assignment.fallback.len());
    println!("{:<34} {:>5} files", "ignored", assignment.ignored);
    Ok(())
}

fn compare(
    repo: &Path,
    config: &Config,
    claims: &[Vec<String>],
    before: &adapter::Snapshot,
    after: &adapter::Snapshot,
    args: &Args,
) -> Result<()> {
    let (before, mut notes) = read(repo, config, claims, before)?;
    let (after, mut later) = read(repo, config, claims, after)?;
    notes.append(&mut later);

    let matched = match_snapshots(before, after);
    let review = review(&matched.definitions, &matched.references);
    let ordering = order(&review, &matched.definitions);

    if args.json {
        let _ = writeln!(
            std::io::stdout().lock(),
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "review": review,
                "ordering": ordering,
            }))?
        );
    } else {
        report::print(&review, &ordering, &matched.definitions, &notes);
        if !args.list && std::io::stdout().is_terminal() {
            walk::walk(&review, &ordering, &matched.definitions)?;
        }
    }
    Ok(())
}

/// Every extractor's answer for one snapshot, plus the leftovers, merged into the one
/// pile of facts the core expects.
fn read(
    repo: &Path,
    config: &Config,
    claims: &[Vec<String>],
    snapshot: &adapter::Snapshot,
) -> Result<(Extraction, Vec<Note>)> {
    let dir = &snapshot.dir;
    let assignment = assign::assign(
        &config.review.ignore,
        claims,
        dir,
        snapshot.files.as_deref(),
    )?;
    let mut merged = fallback::extract(dir, &assignment.fallback);
    let mut notes = Vec::new();

    for (extractor, files) in config.extractors.iter().zip(&assignment.extractors) {
        if files.is_empty() {
            continue;
        }
        let (mut extracted, mut said) = adapter::extract(repo, extractor, dir, files)?;
        merged.occurrences.append(&mut extracted.occurrences);
        merged.mentions.append(&mut extracted.mentions);
        notes.append(&mut said);
    }

    Ok((merged, notes))
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
                files: None,
            })
        }
    }
}

fn clean_up(snapshot: &adapter::Snapshot) {
    if snapshot.temporary {
        let _ = std::fs::remove_dir_all(&snapshot.dir);
    }
}
