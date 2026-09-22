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

use anyhow::{Context, Result, bail};
use config::Config;
use dagger_core::group::Grouping;
use dagger_core::matching::{Extraction, match_snapshots};
use dagger_core::review::{Impact, Warning, review};
use dagger_protocol::Note;
use std::io::{IsTerminal, Write};
use std::path::Path;

struct Args {
    /// What was typed after the flags, in order and untouched. Empty means the user
    /// named nothing, so the snapshot adapter gets to choose. Dagger never reads these:
    /// what counts as a way of naming history is the adapter's to say.
    asked: Vec<String>,
    json: bool,
    explain: bool,
    list: bool,
    help: bool,
    /// How far past what changed to follow what depends on it. Left unsaid, the repository
    /// decides, and failing that so does dagger.
    ripples: Option<u32>,
}

fn parse_args() -> Result<Args> {
    let mut positional = Vec::new();
    let mut json = false;
    let mut explain = false;
    let mut list = false;
    let mut help = false;
    let mut ripples = None;
    let mut awaiting = false;

    for arg in std::env::args().skip(1) {
        if awaiting {
            ripples = Some(
                arg.parse()
                    .with_context(|| format!("--ripples wants a number, not {arg}"))?,
            );
            awaiting = false;
            continue;
        }
        match arg.as_str() {
            "--ripples" => awaiting = true,
            "--json" => json = true,
            "--explain" => explain = true,
            "--list" => list = true,
            // Answered once the repository has been read, since half the answer is the
            // snapshot adapter's and this doesn't know yet which one that is.
            "-h" | "--help" => help = true,
            flag if flag.starts_with('-') => bail!("don't know the flag {flag}"),
            value => positional.push(value.to_string()),
        }
    }

    if awaiting {
        bail!("--ripples wants a number after it");
    }

    Ok(Args {
        asked: positional,
        help,
        ripples,
        json,
        explain,
        list,
    })
}

/// What dagger does, and how this repository lets a change be named.
///
/// The second half isn't dagger's to write. Which words work here depends on the snapshot
/// adapter configured, so it's asked rather than guessed at — a list kept in two places is
/// a list that goes wrong in one of them. A repository with no adapter, or one that won't
/// answer, simply has nothing extra to say.
fn help(repo: &Path, config: &Config) {
    println!("dagger [--json] [--explain] [--list] [--ripples <n>] [what to read]");
    println!();
    println!("Named nothing, dagger reads whatever you're working on.");
    println!();
    println!("--ripples <n> follows what a change reaches n steps out. Nought reads only");
    println!("what changed. Each step costs, so raise it knowingly. A repository can say");
    println!("where to start under [review] as ripples = <n>; failing both, it is 1.");
    println!();

    let understood = config.snapshots.as_ref().and_then(|snapshots| {
        let said = adapter::describe(
            repo,
            &snapshots.adapter,
            &snapshots.args,
            &snapshots.settings,
        );
        said.ok().filter(|said| !said.usage.is_empty())
    });

    match understood {
        Some(said) => {
            println!("What else you can name here:");
            println!();
            for line in said.usage {
                println!("  dagger {line}");
            }
        }
        None => println!("What else you can name depends on the snapshot adapter configured."),
    }
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

    if args.help {
        help(&repo, &config);
        return Ok(());
    }

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
    match (args.asked.as_slice(), &config.snapshots) {
        ([], _) => {}
        (asked, Some(snapshots)) => {
            let found = adapter::revisions(repo, snapshots, asked)?;
            return Ok((found.before, found.after));
        }
        // Without an adapter there's nobody to ask, so two directories is all this can be.
        ([before, after], None) => return Ok((before.clone(), after.clone())),
        (_, None) => bail!(
            "no snapshot adapter is configured in {}, so name two directories",
            config::FILE
        ),
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

    /* What was asked for, else what the repository asks for, else far enough to answer
     * "who breaks if this breaks" and no further. */
    let ripples = args.ripples.or(config.review.ripples).unwrap_or(1);

    let (before_dir, after_dir) = (before.1.dir.clone(), after.1.dir.clone());

    /* One after the other, though neither reading looks at the other and both together are
     * nearly the whole of what a run costs.
     *
     * Reading them at once was tried and taken out again. It ran a third faster and took
     * 24GB to do it: a language server has to load the whole project before it can answer
     * anything, so two of them is two of everything, and on a sixty gigabyte machine it
     * came within a fifth of a percent of what the out-of-memory killer watches for. A
     * review that might be killed partway is worse than a review that takes longer. */
    let (before, mut notes) = read(repo, config, claims, before, &changed, ripples, "before")?;
    let (after, mut later) = read(repo, config, claims, after, &changed, ripples, "after")?;
    notes.append(&mut later);

    let matched = match_snapshots(before, after);

    // Which group each definition is in. Only this side can answer it, since it means
    // looking for marker files on disk, so it's worked out here and handed over.
    let grouping = match config.grouping.as_ref() {
        Some(wanted) => grouping::of(wanted, &after_dir, &matched.definitions),
        None => Grouping::default(),
    };

    /* Everything anyone had to say before the review was worked out: what the adapters
     * couldn't do, and what the two snapshots turned out to disagree about. Dagger's own
     * findings join them inside. */
    let warnings: Vec<Warning> = notes
        .into_iter()
        .map(|note| Warning {
            // An adapter's note is always about something it couldn't do, so whatever it
            // was about isn't in the review.
            impact: Impact::Incomplete,
            message: match &note.file {
                Some(file) => format!("{file}: {}", note.message),
                None => note.message.clone(),
            },
            about: None,
        })
        .collect();
    let mut found = matched.diagnostics;
    found.extend(completeness::check(
        &before_dir,
        &after_dir,
        &changed,
        &matched.definitions,
    ));

    let review = review(
        matched.definitions,
        &matched.references,
        ripples,
        &grouping,
        warnings,
        found,
    );

    if args.json {
        let _ = writeln!(
            std::io::stdout().lock(),
            "{}",
            serde_json::to_string_pretty(&review)?
        );
    } else {
        report::print(&review);
        if !args.list && std::io::stdout().is_terminal() {
            walk::walk(&review)?;
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
    ripples: u32,
    // Which of the two this is. Both are read at once, so everything said on the way has
    // to say whose it is or the two reports become one nobody can follow.
    side: &str,
) -> Result<(Extraction, Vec<Note>)> {
    let dir = &snapshot.dir;
    let assignment = assign::assign(
        &config.review.ignore,
        claims,
        dir,
        snapshot.files.as_deref(),
    )?;
    /* Only the files that differ. Whatever nobody claimed gets read whole, and reading a
     * file that didn't change buys nothing: both sides come out identical, so it's kept,
     * unchanged, and never worth reading. On a repository of any size that's the whole cost
     * of the run — a hundred thousand files read off disk twice to say nothing. */
    let differs: std::collections::BTreeSet<&str> = changed.iter().map(String::as_str).collect();
    let fallen: Vec<String> = assignment
        .fallback
        .iter()
        .filter(|file| differs.contains(file.as_str()))
        .cloned()
        .collect();
    let mut merged = fallback::extract(dir, &fallen);
    let mut notes = Vec::new();

    for (extractor, files) in config.extractors.iter().zip(&assignment.extractors) {
        if files.is_empty() {
            continue;
        }
        status(&format!(
            "{side} · reading {} files of {rev} with {}",
            files.len(),
            extractor.adapter
        ));
        let (mut extracted, mut said) =
            adapter::extract(repo, extractor, dir, files, changed, ripples, side)?;
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
