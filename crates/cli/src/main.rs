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
use dagger_core::model::Span;
use dagger_core::review::{Impact, Warning, review};
use dagger_protocol::{Changed, Note, Revisions};
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
    let revisions = revisions(&repo, &config, &args)?;
    status(&format!(
        "comparing {} to {}",
        revisions.before, revisions.after
    ));

    let before = lay_out(&repo, &config, &revisions.before)?;
    let after = lay_out(&repo, &config, &revisions.after)?;

    // The snapshots take themselves away when they go out of scope here, whichever way
    // this ends.
    if args.explain {
        explain(&config, &claims, &after)
    } else {
        compare(
            &repo,
            &config,
            &claims,
            (&revisions.before, &before),
            (&revisions.after, &after),
            &args,
            revisions.title,
        )
    }
}

/// What to compare. The user's word first, then whatever the snapshot adapter thinks is
/// worth looking at, and failing both, the last commit.
fn revisions(repo: &Path, config: &Config, args: &Args) -> Result<Revisions> {
    match (args.asked.as_slice(), &config.snapshots) {
        ([], _) => {}
        (asked, Some(snapshots)) => return adapter::revisions(repo, snapshots, asked),
        // Without an adapter there's nobody to ask, so two directories is all this can be.
        ([before, after], None) => {
            return Ok(Revisions {
                before: before.clone(),
                after: after.clone(),
                title: None,
            });
        }
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

    Ok(suggested.unwrap_or_else(|| Revisions {
        before: "HEAD~1".to_string(),
        after: "HEAD".to_string(),
        title: None,
    }))
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
    title: Option<String>,
) -> Result<()> {
    let changed = assign::not_ignored(&config.review.ignore, differing(before.1, after.1)?)?;
    status(&format!("{} files differ", changed.len()));

    /* What was asked for, else what the repository asks for, else far enough to answer
     * "who breaks if this breaks" and no further. */
    let ripples = args.ripples.or(config.review.ripples).unwrap_or(1);

    let (before_dir, after_dir) = (before.1.dir.clone(), after.1.dir.clone());

    /* Where, in each differing file, the bytes actually differ — worked out once, on
     * disk, before anything is asked of an adapter. Each side keeps only its own half:
     * an adapter reading `before_dir` has no use for where a line landed in `after_dir`. */
    let (changed_before, changed_after) = changed_ranges(&before_dir, &after_dir, &changed);

    /* One after the other, though neither reading looks at the other and both together are
     * nearly the whole of what a run costs.
     *
     * Reading them at once was tried and taken out again. It ran a third faster and took
     * 24GB to do it: a language server has to load the whole project before it can answer
     * anything, so two of them is two of everything, and on a sixty gigabyte machine it
     * came within a fifth of a percent of what the out-of-memory killer watches for. A
     * review that might be killed partway is worse than a review that takes longer. */
    let (before, mut notes) = read(
        repo,
        config,
        claims,
        before,
        &changed_before,
        ripples,
        "before",
    )?;
    let (after, mut later) = read(
        repo,
        config,
        claims,
        after,
        &changed_after,
        ripples,
        "after",
    )?;
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
        title,
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

/// Where, in each differing file, the bytes actually differ — for the before side and the
/// after side in turn.
///
/// Whole files used to be handed to an adapter as "this one changed", which left it no way
/// to tell a four-line edit from a rewrite: it asked after every definition in the file
/// either way. On redo's own self-review that meant 494 definitions asked about across
/// files that between them had a few dozen lines actually differ.
///
/// A file the line diff finds nothing to narrow — its bytes differ, or it wouldn't be here,
/// but not in a way lines can say, a binary file being the usual reason — gets no ranges,
/// which an adapter reads as "the whole file", the same as before this existed.
fn changed_ranges(before: &Path, after: &Path, files: &[String]) -> (Vec<Changed>, Vec<Changed>) {
    let mut on_before = Vec::with_capacity(files.len());
    let mut on_after = Vec::with_capacity(files.len());

    for file in files {
        let was = std::fs::read_to_string(before.join(file)).unwrap_or_default();
        let now = std::fs::read_to_string(after.join(file)).unwrap_or_default();
        let (at_before, at_after) = ranges(&was, &now);
        on_before.push(Changed {
            file: file.clone(),
            at: at_before,
        });
        on_after.push(Changed {
            file: file.clone(),
            at: at_after,
        });
    }

    (on_before, on_after)
}

/// The stretches of each side a line diff didn't find equal, as byte spans rather than
/// line numbers — which is what a definition's own span is written in, and the only
/// currency the two can be compared in.
fn ranges(before: &str, after: &str) -> (Vec<Span>, Vec<Span>) {
    let starts = |text: &str| -> Vec<u32> {
        let mut at = vec![0u32];
        at.extend(text.match_indices('\n').map(|(pos, _)| pos as u32 + 1));
        at
    };
    let (was, is) = (starts(before), starts(after));
    let span = |starts: &[u32], text: &str, from: usize, to: usize| -> Span {
        Span {
            start: starts[from],
            end: starts.get(to).copied().unwrap_or(text.len() as u32),
        }
    };
    /* Where an insertion or a deletion sits on the side that has no lines to show for
     * it — a point, not a stretch, since nothing there differs. It still falls inside
     * whatever definition encloses it: an interface gaining a field is a changed
     * interface on both sides, even though only one side has a line to point at. Left
     * unmarked, that side never asks after the interface's contract, the other side
     * does, and the two readings disagree about something neither of them got wrong. */
    let seam = |starts: &[u32], text: &str, at: usize| -> Span {
        let point = starts.get(at).copied().unwrap_or(text.len() as u32);
        Span {
            start: point,
            end: point,
        }
    };

    let (mut at_before, mut at_after) = (Vec::new(), Vec::new());
    for op in similar::TextDiff::from_lines(before, after).ops() {
        use similar::DiffOp::*;
        match *op {
            Equal { .. } => {}
            Delete {
                old_index,
                old_len,
                new_index,
            } => {
                at_before.push(span(&was, before, old_index, old_index + old_len));
                at_after.push(seam(&is, after, new_index));
            }
            Insert {
                old_index,
                new_index,
                new_len,
            } => {
                at_before.push(seam(&was, before, old_index));
                at_after.push(span(&is, after, new_index, new_index + new_len));
            }
            Replace {
                old_index,
                old_len,
                new_index,
                new_len,
            } => {
                at_before.push(span(&was, before, old_index, old_index + old_len));
                at_after.push(span(&is, after, new_index, new_index + new_len));
            }
        }
    }
    (at_before, at_after)
}

/// Every extractor's answer for one snapshot, plus the leftovers, merged into the one
/// pile of facts the core expects.
fn read(
    repo: &Path,
    config: &Config,
    claims: &[Vec<String>],
    (rev, snapshot): (&str, &adapter::Snapshot),
    changed: &[Changed],
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
    let differs: std::collections::BTreeSet<&str> =
        changed.iter().map(|one| one.file.as_str()).collect();
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Whether any span covers this byte position — what an adapter does with a seam to
    /// decide whether the definition sitting there is worth asking about.
    fn covered(spans: &[Span], at: u32) -> bool {
        spans
            .iter()
            .any(|span| span.start <= at && at < span.end.max(span.start + 1))
    }

    #[test]
    fn identical_text_has_nothing_to_say() {
        let (before, after) = ranges("fn one() {}\n", "fn one() {}\n");
        assert!(before.is_empty() && after.is_empty());
    }

    #[test]
    fn a_changed_line_is_marked_on_both_sides() {
        let (before, after) = ranges("let a = 1;\n", "let a = 2;\n");
        assert_eq!(before.len(), 1);
        assert_eq!(after.len(), 1);
        assert!(covered(&before, 4)); // somewhere inside "let a = 1;"
        assert!(covered(&after, 4));
    }

    /* The case that used to go missing: a line arrives with nothing removed to pair it
     * with, so the side that lost nothing got no range at all — and a definition whose
     * braces span the insertion point never learned it had changed. */
    #[test]
    fn a_pure_insertion_still_marks_a_seam_on_the_other_side() {
        let before = "interface Money {\n  amount: number;\n}\n";
        let after = "interface Money {\n  amount: number;\n  precise: boolean;\n}\n";
        let (before_at, after_at) = ranges(before, after);

        assert!(
            !before_at.is_empty(),
            "the side with nothing removed still needs a seam"
        );
        assert!(!after_at.is_empty());

        // The seam sits between "amount: number;" and the closing brace — inside the
        // interface's own span either way it's measured.
        let whole = 0..before.len() as u32;
        assert!(before_at.iter().all(|span| whole.contains(&span.start)));
    }

    #[test]
    fn a_pure_deletion_mirrors_the_same_seam() {
        let before = "interface Money {\n  amount: number;\n  precise: boolean;\n}\n";
        let after = "interface Money {\n  amount: number;\n}\n";
        let (before_at, after_at) = ranges(before, after);

        assert!(!before_at.is_empty());
        assert!(
            !after_at.is_empty(),
            "the side with nothing added still needs a seam"
        );
    }

    /* Two separate hunks, apart in the file, stay apart rather than merging into one
     * span that would claim everything between them as changed too. */
    #[test]
    fn separate_hunks_are_reported_separately() {
        let before = "fn a() { 1 }\nfn mid() { 0 }\nfn b() { 2 }\n";
        let after = "fn a() { 10 }\nfn mid() { 0 }\nfn b() { 20 }\n";
        let (before_at, _) = ranges(before, after);
        assert_eq!(
            before_at.len(),
            2,
            "the untouched middle line shouldn't join them"
        );
    }
}
