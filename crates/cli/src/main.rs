//! Drives the adapters, hands the results to the core, and prints what came back.
//!
//! Everything that touches a disk or starts a process lives on this side. The core is
//! given two piles of facts and nothing else.

mod adapter;
mod assign;
mod completeness;
mod config;
mod diff;
mod fallback;
mod grouping;
mod report;
mod walk;

use anyhow::{Result, bail};
use config::Config;
use dagger_core::group::Grouping;
use dagger_core::matching::{Extraction, match_snapshots};
use dagger_core::order::order;
use dagger_core::review::review;
use dagger_protocol::Note;
use std::io::{IsTerminal, Write};
use std::path::Path;

struct Args {
    /// Left alone when the user named nothing, so the snapshot adapter gets to choose.
    revisions: Option<(String, String)>,
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

    let revisions = match positional.as_slice() {
        [] => None,
        [before, after] => Some((before.clone(), after.clone())),
        _ => bail!("expected two revisions, or none at all"),
    };

    Ok(Args {
        revisions,
        json,
        explain,
        list,
    })
}

/// Progress goes to stderr, where it can't get mixed into the review itself. Reading two
/// snapshots takes long enough that saying nothing looks like a hang.
fn status(saying: &str) {
    let mut err = std::io::stderr();
    let _ = if err.is_terminal() {
        writeln!(err, "\x1b[2m{saying}\x1b[0m")
    } else {
        writeln!(err, "{saying}")
    };
}

fn main() -> Result<()> {
    let args = parse_args()?;
    let repo = std::env::current_dir()?;
    let config = Config::read(&repo)?;

    let claims = claims(&repo, &config)?;
    let (before_rev, after_rev) = revisions(&repo, &config, &args)?;
    status(&format!("comparing {before_rev} to {after_rev}"));

    let before = lay_out(&repo, &config, &before_rev)?;
    let after = lay_out(&repo, &config, &after_rev)?;

    // The snapshots take themselves away when they go out of scope here, whichever way
    // this ends.
    if args.explain {
        explain(&config, &claims, &after)
    } else {
        compare(
            &repo,
            &config,
            &claims,
            (&before_rev, &before),
            (&after_rev, &after),
            &args,
        )
    }
}

/// What to compare. The user's word first, then whatever the snapshot adapter thinks is
/// worth looking at, and failing both, the last commit.
fn revisions(repo: &Path, config: &Config, args: &Args) -> Result<(String, String)> {
    if let Some(named) = &args.revisions {
        return Ok(named.clone());
    }

    let suggested = match &config.snapshots {
        Some(snapshots) => {
            adapter::describe(
                repo,
                &snapshots.adapter,
                &snapshots.args,
                &snapshots.settings,
            )?
            .revisions
        }
        None => None,
    };

    Ok(match suggested {
        Some(revisions) => (revisions.before, revisions.after),
        None => ("HEAD~1".to_string(), "HEAD".to_string()),
    })
}

/// What each extractor will be given: what the repo asked for, or failing that, what
/// the adapter says it reads.
fn claims(repo: &Path, config: &Config) -> Result<Vec<Vec<String>>> {
    config
        .extractors
        .iter()
        .map(|extractor| {
            if extractor.include.is_empty() {
                Ok(adapter::describe(
                    repo,
                    &extractor.adapter,
                    &extractor.args,
                    &extractor.settings,
                )?
                .include)
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
    before: (&str, &adapter::Snapshot),
    after: (&str, &adapter::Snapshot),
    args: &Args,
) -> Result<()> {
    let changed = assign::not_ignored(&config.review.ignore, differing(before.1, after.1)?)?;
    status(&format!("{} files differ", changed.len()));

    let (before_dir, after_dir) = (before.1.dir.clone(), after.1.dir.clone());
    let (before, mut notes) = read(repo, config, claims, before, &changed)?;
    let (after, mut later) = read(repo, config, claims, after, &changed)?;
    notes.append(&mut later);

    let matched = match_snapshots(before, after);
    let mut review = review(&matched.definitions, &matched.references);

    // One grouping at a time, and for now the first one written down. The reading order
    // leans on it to know whether the next definition takes the reader somewhere else.
    let grouping = match config.groupings.first() {
        Some(wanted) => grouping::of(wanted, &after_dir, &matched.definitions),
        None => Grouping::default(),
    };
    let ordering = order(&review, &matched.definitions, &grouping);

    review.diagnostics.extend(completeness::check(
        &before_dir,
        &after_dir,
        &changed,
        &matched.definitions,
    ));

    if args.json {
        // The definitions belong in the output too: everything else talks in identities,
        // and without these there's nothing to turn one back into a name, a file, or the
        // text a reader came to see.
        //
        // Only what the page will draw, though — the members and the modules around them.
        // Handing over every definition means handing over the whole repository twice, since
        // the fallback reading holds a copy of each file it covers whether anything changed
        // in it or not.
        let shown: Vec<_> = matched
            .definitions
            .iter()
            .filter(|definition| {
                review.members.contains(&definition.identity)
                    || review.context.contains(&definition.identity)
            })
            .collect();

        let _ = writeln!(
            std::io::stdout().lock(),
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "definitions": shown,
                "review": review,
                "ordering": ordering,
                "grouping": grouping,
                "notes": notes,
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

/// Which files aren't the same on both sides. Only a hint for adapters, so a file
/// that can't be read counts as differing rather than stopping anything.
fn differing(before: &adapter::Snapshot, after: &adapter::Snapshot) -> Result<Vec<String>> {
    let listed = |snapshot: &adapter::Snapshot| -> Result<Vec<String>> {
        match &snapshot.files {
            Some(files) => Ok(files.clone()),
            None => assign::walk_all(&snapshot.dir),
        }
    };

    let mut names: Vec<String> = listed(before)?;
    names.extend(listed(after)?);
    names.sort();
    names.dedup();

    Ok(names
        .into_iter()
        .filter(|name| {
            let one = std::fs::read(before.dir.join(name)).ok();
            let other = std::fs::read(after.dir.join(name)).ok();
            one != other
        })
        .collect())
}

/// Every extractor's answer for one snapshot, plus the leftovers, merged into the one
/// pile of facts the core expects.
fn read(
    repo: &Path,
    config: &Config,
    claims: &[Vec<String>],
    (rev, snapshot): (&str, &adapter::Snapshot),
    changed: &[String],
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
        status(&format!(
            "reading {} files of {rev} with {}",
            files.len(),
            extractor.adapter
        ));
        let (mut extracted, mut said) = adapter::extract(repo, extractor, dir, files, changed)?;
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
