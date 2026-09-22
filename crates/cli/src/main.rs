//! Drives the adapters, hands the results to the core, and prints what came back.
//! Everything that touches disk or starts a process lives here; the core only gets facts.

mod adapter;
mod assign;
mod completeness;
mod config;
mod diff;
mod fallback;
mod grouping;
mod md;
mod open;
mod report;
mod walk;

use anyhow::{Context, Result, bail};
use config::Config;
use dagger_core::group::Grouping;
use dagger_core::matching::{Extraction, match_snapshots};
use dagger_core::model::Span;
use dagger_core::prose::line_starts;
use dagger_core::review::{Impact, Warning, review};
use dagger_protocol::{Changed, Note, Revisions};
use std::collections::BTreeSet;
use std::io::{IsTerminal, Write};
use std::path::Path;

struct Args {
    /// Positional arguments, passed to the snapshot adapter untouched since only it knows
    /// what they mean. Empty means the adapter chooses.
    asked: Vec<String>,
    output: Output,
    explain: bool,
    list: bool,
    help: bool,
    /// How far past what changed to follow dependents. Unset means the repository's
    /// setting, then dagger's default.
    ripples: Option<u32>,
}

impl Args {
    /// Nothing but the mode word, or no mode at all: say how it's used.
    fn help() -> Self {
        Args {
            asked: Vec::new(),
            output: Output::Terminal,
            explain: false,
            list: false,
            help: true,
            ripples: None,
        }
    }
}

/// Where the review goes: the mode word on the command line.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Output {
    Terminal,
    Json,
    Markdown,
}

fn parse_args(given: impl Iterator<Item = String>, output: Output) -> Result<Args> {
    let mut positional = Vec::new();
    let mut explain = false;
    let mut list = false;
    let mut help = false;
    let mut ripples = None;
    let mut awaiting = false;

    for arg in given {
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
            "--explain" => explain = true,
            "--list" => list = true,
            // Answered after reading the config, since the snapshot adapter supplies half the text.
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
        output,
        explain,
        list,
    })
}

/// Prints usage. The snapshot adapter is asked for its own part so the list of ways to
/// name a change isn't kept in two places.
fn help(repo: &Path, config: &Config) {
    println!("dagger cli  [--explain] [--list] [--ripples <n>] [what to read]");
    println!("dagger gui  [--ripples <n>] [what to read]");
    println!("dagger json [--ripples <n>] [what to read]");
    println!("dagger md   [--ripples <n>] [what to read]");
    println!();
    println!("The first word says where the review goes: the terminal, a browser, one line");
    println!("of JSON, or markdown for an agent. Named nothing, all read what you're working on.");
    println!();
    println!("The built-in adapters run as subcommands: dagger git, dagger lsp, dagger rust.");
    println!("Name one as `adapter = \"git\"` in dagger.toml, or leave the file out and dagger");
    println!("picks them from what the repository holds.");
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

/// Progress goes to stderr so it can't mix into the review. Saying nothing looks like a hang.
fn status(saying: &str) {
    let mut err = std::io::stderr();
    let _ = if err.is_terminal() {
        writeln!(err, "\x1b[2m{saying}\x1b[0m")
    } else {
        writeln!(err, "{saying}")
    };
}

fn main() -> Result<()> {
    let mut given = std::env::args().skip(1);
    let repo = std::env::current_dir()?;
    // The same words follow every mode; only where the review goes differs.
    let args = match given.next().as_deref() {
        Some("git") => return dagger_protocol::serve(dagger_git_adapter::answer),
        Some("lsp") => return dagger_protocol::serve(dagger_lsp_adapter::answer),
        Some("rust") => return dagger_protocol::serve(dagger_rust_adapter::answer),
        Some("cli") => parse_args(given, Output::Terminal)?,
        Some("json") => parse_args(given, Output::Json)?,
        Some("md") => parse_args(given, Output::Markdown)?,
        Some("gui") => {
            let words: Vec<String> = given.collect();
            let args = parse_args(words.iter().cloned(), Output::Terminal)?;
            if args.list || args.explain {
                bail!("--list and --explain are terminal output; use dagger cli");
            }
            if !args.help {
                return open::serve(&repo, words);
            }
            args
        }
        _ => Args::help(),
    };
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

    // Temporary snapshots are removed on drop, whichever way this ends.
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

/// What to compare: the user's choice, else the snapshot adapter's suggestion, else the last commit.
fn revisions(repo: &Path, config: &Config, args: &Args) -> Result<Revisions> {
    match (args.asked.as_slice(), &config.snapshots) {
        ([], _) => {}
        (asked, Some(snapshots)) => return adapter::revisions(repo, snapshots, asked),
        // Without an adapter the arguments can only be two directories.
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

/// Globs per extractor: the configured `include`, or failing that what the adapter declares.
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

/// Prints which extractor got which files, so a surprising assignment can be checked.
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
        let source = if extractor.inferred {
            "inferred"
        } else if extractor.include.is_empty() {
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

    // The default of 1 is just enough to answer "who breaks if this breaks".
    let ripples = args.ripples.or(config.review.ripples).unwrap_or(1);

    let (before_dir, after_dir) = (before.1.dir.clone(), after.1.dir.clone());

    // Worked out once here so adapters don't each diff the files. Each side only gets its
    // own half, since an adapter reading one directory has no use for the other's offsets.
    let (changed_before, changed_after) = changed_ranges(&before_dir, &after_dir, &changed);

    // Read sequentially on purpose. Reading both at once means two language servers each
    // loading the whole project, which took 24GB and nearly hit the OOM killer.
    let (before, notes) = read(
        repo,
        config,
        claims,
        before,
        &changed_before,
        ripples,
        "before",
    )?;
    let (after, later) = read(
        repo,
        config,
        claims,
        after,
        &changed_after,
        ripples,
        "after",
    )?;

    let matched = match_snapshots(before, after);

    // Grouping needs marker files on disk, so it's worked out here rather than in the core.
    let grouping = match config.grouping.as_ref() {
        Some(wanted) => grouping::of(wanted, &after_dir, &matched.definitions),
        None => Grouping::default(),
    };

    let warnings: Vec<Warning> = notes
        .into_iter()
        .chain(later)
        .map(|note| Warning {
            // An adapter's note is always about something it couldn't do.
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

    match args.output {
        // One line, so whoever is reading can take it the moment it ends rather than
        // waiting for this process to finish removing its snapshots.
        Output::Json => {
            let _ = writeln!(
                std::io::stdout().lock(),
                "{}",
                serde_json::to_string(&review)?
            );
        }
        Output::Markdown => print!("{}", md::render(&review)),
        Output::Terminal => {
            report::print(&review);
            if !args.list && std::io::stdout().is_terminal() {
                walk::walk(&review)?;
            }
        }
    }
    Ok(())
}

/// Files that differ between sides. Only a hint for adapters, so an unreadable file
/// counts as differing rather than failing.
fn differing(before: &adapter::Snapshot, after: &adapter::Snapshot) -> Result<Vec<String>> {
    let listed = |snapshot: &adapter::Snapshot| -> Result<Vec<String>> {
        match &snapshot.files {
            Some(files) => Ok(files.clone()),
            None => assign::walk_all(&snapshot.dir),
        }
    };

    let names: BTreeSet<String> = listed(before)?.into_iter().chain(listed(after)?).collect();

    Ok(names
        .into_iter()
        .filter(|name| {
            let one = std::fs::read(before.dir.join(name)).ok();
            let other = std::fs::read(after.dir.join(name)).ok();
            one != other
        })
        .collect())
}

/// Byte ranges that differ in each file, per side, so an adapter can skip definitions a
/// small edit didn't touch. A file the line diff can't narrow (a binary, usually) gets no
/// ranges, which an adapter reads as "the whole file".
fn changed_ranges(before: &Path, after: &Path, files: &[String]) -> (Vec<Changed>, Vec<Changed>) {
    files
        .iter()
        .map(|file| {
            let was = std::fs::read_to_string(before.join(file)).unwrap_or_default();
            let now = std::fs::read_to_string(after.join(file)).unwrap_or_default();
            let (at_before, at_after) = ranges(&was, &now);
            (
                Changed {
                    file: file.clone(),
                    at: at_before,
                },
                Changed {
                    file: file.clone(),
                    at: at_after,
                },
            )
        })
        .unzip()
}

/// The unequal stretches of each side, as byte spans since that's what definition spans use.
fn ranges(before: &str, after: &str) -> (Vec<Span>, Vec<Span>) {
    let (was, is) = (line_starts(before), line_starts(after));
    let span = |starts: &[usize], text: &str, from: usize, to: usize| -> Span {
        Span {
            start: starts[from] as u32,
            end: starts.get(to).copied().unwrap_or(text.len()) as u32,
        }
    };
    // A zero-width span on the side that has no lines for an insertion or deletion. The
    // enclosing definition still has to count as changed on both sides, or the two
    // readings disagree.
    let seam = |starts: &[usize], text: &str, at: usize| -> Span {
        let point = starts.get(at).copied().unwrap_or(text.len()) as u32;
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

/// Every extractor's answer for one snapshot, plus the fallback, merged into one extraction.
fn read(
    repo: &Path,
    config: &Config,
    claims: &[Vec<String>],
    (rev, snapshot): (&str, &adapter::Snapshot),
    changed: &[Changed],
    ripples: u32,
    // Labels progress output so the two readings can be told apart.
    side: &str,
) -> Result<(Extraction, Vec<Note>)> {
    let dir = &snapshot.dir;
    let assignment = assign::assign(
        &config.review.ignore,
        claims,
        dir,
        snapshot.files.as_deref(),
    )?;
    // Only files that differ: an unchanged file reads the same on both sides, and reading a
    // whole repository twice to say nothing was most of the run's cost.
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
        // An extractor none of whose files changed has nothing to say: references never
        // cross from one extractor's files into another's, so nothing of its was reached.
        if !files.iter().any(|file| differs.contains(file.as_str())) {
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

/// Without a snapshot adapter, a revision is just a directory.
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

    /// Whether any span covers this byte, the way an adapter checks a seam.
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

        // The seam must land inside the interface's own span.
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
