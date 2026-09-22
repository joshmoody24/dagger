//! Lays a git revision out on disk for the extractors to read.
//!
//! Runs in the repository it's reading, since a request only names a revision. Uses
//! `git archive` rather than a checkout, so the user's working tree and index are
//! never touched.
//!
//! The revision `current` means the files as they are right now, uncommitted edits
//! and new files included. That one needs no copying: the repository is already the
//! snapshot, and we just say which files count so that build output stays out.

use anyhow::{Context, Result, bail};
use dagger_protocol::{Request, Response, Revisions};
use serde::Deserialize;
use std::collections::BTreeSet;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn main() -> Result<()> {
    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;
    let request: Request = serde_json::from_str(&input).context("that isn't a dagger request")?;

    let response = match answer(request) {
        Ok(response) => response,
        Err(error) => Response::Failed {
            message: format!("{error:#}"),
        },
    };

    println!("{}", serde_json::to_string(&response)?);
    Ok(())
}

/// What a repo can tell this adapter.
#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Settings {
    /// Point the snapshot at the repository's ignored files — build output, installed
    /// packages — instead of leaving them out. On by default: without them, tooling that
    /// reads generated declarations has to compile everything from source instead.
    carry_ignored: bool,
    /// What branches are cut from and merged back into, when it isn't the obvious one.
    /// Left empty, this is whatever the remote says its own HEAD is.
    trunk: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            carry_ignored: true,
            trunk: String::new(),
        }
    }
}

fn answer(request: Request) -> Result<Response> {
    match request {
        Request::Materialize { rev, .. } if rev == CURRENT => Ok(Response::Materialized {
            dir: std::env::current_dir()?.to_string_lossy().into_owned(),
            temporary: false,
            files: Some(current_files()?),
        }),
        Request::Materialize { rev, settings } => {
            let settings = settings_of(settings)?;
            let dir = materialize(&rev, settings.carry_ignored)?;
            let files = Some(listing(&["ls-tree", "-r", "--name-only", "-z", &rev])?);
            Ok(Response::Materialized {
                dir: dir.to_string_lossy().into_owned(),
                temporary: true,
                files,
            })
        }
        Request::Describe { settings } => Ok(Response::Described {
            include: {
                // Nothing here needs them, but this is the first thing dagger asks, and a
                // setting nobody understands is worth hearing about before a snapshot has
                // been laid out rather than after.
                settings_of(settings)?;
                Vec::new()
            },
            revisions: Some(worth_reviewing()?),
            usage: UNDERSTOOD.lines().map(str::to_string).collect(),
        }),
        Request::Resolve { asked, settings } => {
            let settings = settings_of(settings)?;
            Ok(Response::Resolved {
                revisions: resolve(&asked, &settings)?,
            })
        }
        Request::Extract { .. } => bail!("git only lays snapshots out, it doesn't read them"),
    }
}

/// What the repository told this adapter, refused if it isn't something this adapter
/// knows.
///
/// A setting quietly ignored is worse than one rejected: the run carries on, answers a
/// question nobody asked, and looks exactly like a run that did as it was told. This used
/// to take whatever it could make sense of and shrug off the rest, which meant a typo in
/// `dagger.toml` was invisible — and so was every setting written after it.
fn settings_of(settings: serde_json::Value) -> Result<Settings> {
    if settings.is_null() {
        return Ok(Settings::default());
    }
    serde_json::from_value(settings)
        .context("dagger-git was told something under settings that it doesn't know")
}

/// Not a revision git knows about, so we answer it ourselves.
const CURRENT: &str = "current";

/// Unfinished work is what someone is most likely to want to look at, so that wins
/// when there is any. Failing that, the last thing they committed.
fn worth_reviewing() -> Result<Revisions> {
    let dirty = !listing(&["status", "--porcelain", "-z"])?.is_empty();
    Ok(if dirty {
        Revisions {
            before: "HEAD".to_string(),
            after: CURRENT.to_string(),
        }
    } else {
        Revisions {
            before: "HEAD~1".to_string(),
            after: "HEAD".to_string(),
        }
    })
}

/// The two ends of what someone asked for, before git has been asked to resolve them.
#[derive(Debug, PartialEq)]
struct Ends<'a> {
    left: &'a str,
    right: &'a str,
    /// Whether the left end means "where these two parted" rather than the revision
    /// itself — git's `...`, and what reviewing a branch wants.
    parted: bool,
}

/// The trunk, spelled so `resolve` knows to go and find it.
const TRUNK: &str = "";

/// Every way this adapter lets a change be named. Said once: dagger shows it in its own
/// help, and it's what a reader who typed something else gets told.
const UNDERSTOOD: &str = "\
branch <name>        that branch, since it left the trunk
commits <a> <b>      those two revisions
commits <a>..<b>     or <a>...<b>, as git writes them";

/// What was typed, in the words this adapter knows.
///
/// Spelled out rather than guessed at. A single name could just as well mean "the branch
/// I want to read" as "the thing my branch came from", and the two give entirely different
/// answers — one of them quietly, since a review of the wrong change looks exactly like a
/// review of the right one. Git learned this with `checkout` and split it in two, so
/// there's no sense learning it again here.
fn read(asked: &[String]) -> Result<Ends<'_>> {
    match asked {
        [word, name] if word == "branch" => Ok(Ends {
            left: TRUNK,
            right: name,
            parted: true,
        }),
        [word, range] if word == "commits" && range.contains("..") => Ok(span(range)),
        [word, before, after] if word == "commits" => Ok(Ends {
            left: before,
            right: after,
            parted: false,
        }),
        [range] if range.contains("..") => Ok(span(range)),
        _ => bail!("dagger-git doesn't know what that means. It understands:\n{UNDERSTOOD}"),
    }
}

/// One of git's own ranges. `...` is where two parted, `..` is the revisions themselves.
fn span(range: &str) -> Ends<'_> {
    let (parted, (left, right)) = match range.split_once("...") {
        // Tried before `..`, which would otherwise read the third dot as part of a name.
        Some(ends) => (true, ends),
        None => (false, range.split_once("..").expect("a range has two dots")),
    };
    Ends {
        left: if left.is_empty() { "HEAD" } else { left },
        right: if right.is_empty() { "HEAD" } else { right },
        parted,
    }
}

/// Where a branch parted from what it branched off, or the revisions themselves.
///
/// Both ends come back as commits rather than as whatever was typed, so that nothing
/// downstream has to resolve a name a second time and possibly differently.
fn resolve(asked: &[String], settings: &Settings) -> Result<Revisions> {
    let Ends {
        left,
        right,
        parted,
    } = read(asked)?;

    let left = if left == TRUNK {
        trunk(settings)?
    } else {
        left.to_string()
    };
    let (left, right) = (commit(&left)?, commit(right)?);

    let before = if parted {
        let found = say(&["merge-base", &left, &right])?;
        if found.is_empty() {
            bail!("those two share no history, so there's nothing between them");
        }
        found
    } else {
        left
    };

    Ok(Revisions {
        before,
        after: right,
    })
}

/// What branches here are cut from.
///
/// A remote records which branch it hands out by default, which is the same question, and
/// having been told once git remembers it. Repositories that never got that far fall back
/// to the usual names.
fn trunk(settings: &Settings) -> Result<String> {
    if !settings.trunk.is_empty() {
        return Ok(settings.trunk.clone());
    }
    if let Ok(named) = say(&["symbolic-ref", "--short", "refs/remotes/origin/HEAD"])
        && !named.is_empty()
    {
        return Ok(named);
    }
    for guess in ["origin/main", "origin/master", "main", "master"] {
        if commit(guess).is_ok() {
            return Ok(guess.to_string());
        }
    }
    bail!(
        "couldn't tell what branches here are cut from. Say so in dagger.toml, under \
         [snapshots.settings] as trunk = \"...\""
    )
}

/// The commit a name stands for, the way a person means it.
///
/// A branch someone fetched but never checked out is only a remote-tracking ref, so the
/// name they read on the pull request isn't a revision git will answer to. `git checkout`
/// guesses past that and everything else refuses to, which is why naming a colleague's
/// branch looks like a typo. This guesses the same way: the name as written first, then
/// the one remote that has it.
fn commit(name: &str) -> Result<String> {
    if let Ok(found) = say(&[
        "rev-parse",
        "--verify",
        "--quiet",
        &format!("{name}^{{commit}}"),
    ]) && !found.is_empty()
    {
        return Ok(found);
    }

    let tracking = lines(&[
        "for-each-ref",
        "--format=%(refname:short)",
        &format!("refs/remotes/*/{name}"),
    ])?;

    match tracking.as_slice() {
        [only] => say(&["rev-parse", "--verify", &format!("{only}^{{commit}}")]),
        [] => bail!(
            "there's no branch, tag or commit called {name} here. If it's someone else's \
             branch, fetch it first"
        ),
        several => bail!(
            "{name} is on more than one remote, so say which: {}",
            several.join(", ")
        ),
    }
}

/// Tracked files plus anything new that isn't ignored, which is the same set git
/// status talks about.
fn current_files() -> Result<Vec<String>> {
    let listed = listing(&[
        "ls-files",
        "--cached",
        "--others",
        "--exclude-standard",
        "-z",
    ])?;

    /* Minus whatever has been deleted but not yet staged. `--cached` means the index, and
     * the index still holds a file somebody has only removed from disk — so the snapshot
     * claimed a file that wasn't there, the extractor was asked to read it, and the review
     * carried "this might not be showing you something" about a deletion it was showing
     * perfectly well. Alarm about nothing teaches a reader to ignore alarm. */
    let gone: BTreeSet<String> = listing(&["ls-files", "--deleted", "-z"])?
        .into_iter()
        .collect();

    Ok(listed
        .into_iter()
        .filter(|file| !gone.contains(file))
        .collect())
}

/// One line of answer, with the newline git puts after it taken off.
fn say(args: &[&str]) -> Result<String> {
    Ok(run(args)?.trim().to_string())
}

/// An answer that comes back a line at a time, rather than NUL-separated.
fn lines(args: &[&str]) -> Result<Vec<String>> {
    Ok(run(args)?
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect())
}

fn run(args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .args(args)
        .output()
        .context("couldn't run git")?;
    if !output.status.success() {
        bail!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8(output.stdout)?)
}

fn listing(args: &[&str]) -> Result<Vec<String>> {
    Ok(run(args)?
        .split('\0')
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .collect())
}

fn materialize(rev: &str, carry_ignored: bool) -> Result<PathBuf> {
    let commit = rev_parse(rev)?;
    let dir = std::env::temp_dir().join(format!("dagger-{}-{}", std::process::id(), commit));
    fs::create_dir_all(&dir).with_context(|| format!("couldn't make {}", dir.display()))?;
    export(&commit, &dir)?;
    if carry_ignored {
        carry(&dir)?;
    }
    Ok(dir)
}

/// Points the snapshot at whatever the repository ignores: build output, installed
/// packages, caches.
///
/// None of it is part of a review — it isn't tracked, so it can't have changed — but
/// leaving it out is the difference between a language server reading one small generated
/// declaration per package and compiling every package from source. On a monorepo that is
/// the difference between seconds and never finishing.
///
/// Links rather than copies, so nothing is duplicated and nothing is written to. What's
/// there belongs to whenever the repository was last built rather than to this revision,
/// which can make a neighbouring package's types slightly out of date. The files being
/// reviewed are read from the snapshot itself and aren't affected.
fn carry(dir: &Path) -> Result<()> {
    let repo = std::env::current_dir()?;

    for entry in listing(&["status", "--porcelain", "--ignored", "-z"])? {
        let Some(path) = entry.strip_prefix("!! ") else {
            continue;
        };
        let path = path.trim_end_matches('/');
        let (target, link) = (repo.join(path), dir.join(path));

        if link.exists() || !target.exists() {
            continue;
        }
        if let Some(parent) = link.parent() {
            fs::create_dir_all(parent)?;
        }
        std::os::unix::fs::symlink(&target, &link)
            .with_context(|| format!("couldn't point {path} at the real one"))?;
    }

    Ok(())
}

fn rev_parse(rev: &str) -> Result<String> {
    let output = Command::new("git")
        .args(["rev-parse", rev])
        .output()
        .context("couldn't run git")?;
    if !output.status.success() {
        bail!(
            "git doesn't know the revision {rev}: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8(output.stdout)?.trim().to_string())
}

/// `git archive | tar -x`, wired up directly so no shell gets involved.
fn export(commit: &str, dir: &Path) -> Result<()> {
    let mut archive = Command::new("git")
        .args(["archive", "--format=tar", commit])
        .stdout(Stdio::piped())
        .spawn()
        .context("couldn't run git archive")?;

    let stdout = archive.stdout.take().expect("stdout was piped");
    let mut extract = Command::new("tar")
        .arg("-x")
        .arg("-C")
        .arg(dir)
        .stdin(stdout)
        .spawn()
        .context("couldn't run tar")?;

    let archived = archive.wait()?;
    let extracted = extract.wait()?;
    if !archived.success() {
        bail!("git archive of {commit} failed");
    }
    if !extracted.success() {
        bail!("unpacking {commit} into {} failed", dir.display());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asked(words: &[&str]) -> Vec<String> {
        words.iter().map(|word| word.to_string()).collect()
    }

    #[test]
    fn a_branch_is_read_against_the_trunk() {
        let words = asked(&["branch", "feature"]);
        let ends = read(&words).unwrap();
        assert_eq!(ends.left, TRUNK);
        assert_eq!(ends.right, "feature");
        assert!(ends.parted, "a branch is read from where it parted");
    }

    #[test]
    fn two_commits_are_those_two() {
        let words = asked(&["commits", "a", "b"]);
        assert_eq!(
            read(&words).unwrap(),
            Ends {
                left: "a",
                right: "b",
                parted: false
            }
        );
    }

    #[test]
    fn gits_own_ranges_are_understood() {
        let two = asked(&["commits", "main..feature"]);
        assert_eq!(
            read(&two).unwrap(),
            Ends {
                left: "main",
                right: "feature",
                parted: false
            }
        );
        let three = asked(&["main...feature"]);
        assert_eq!(
            read(&three).unwrap(),
            Ends {
                left: "main",
                right: "feature",
                parted: true
            }
        );
    }

    /* `...` has to be tried first: read as two dots it would leave a name starting with a
     * dot, and ask git about a revision nobody typed. */
    #[test]
    fn three_dots_are_not_read_as_two() {
        assert_eq!(span("main...HEAD").left, "main");
        assert_eq!(span("main...HEAD").right, "HEAD");
        assert!(span("main...HEAD").parted);
    }

    #[test]
    fn an_end_left_out_is_where_you_are() {
        assert_eq!(span("main..").right, "HEAD");
        assert_eq!(span("..main").left, "HEAD");
    }

    /* The whole point of spelling it out: a name on its own meant two different things
     * depending on who typed it, and got no complaint either way. */
    #[test]
    fn a_bare_name_is_refused_rather_than_guessed_at() {
        let words = asked(&["main"]);
        let said = read(&words).unwrap_err().to_string();
        assert!(
            said.contains("branch <name>"),
            "should say what it knows: {said}"
        );
    }

    #[test]
    fn nonsense_is_refused() {
        for words in [
            asked(&[]),
            asked(&["commits"]),
            asked(&["branch", "a", "b"]),
        ] {
            assert!(read(&words).is_err(), "{words:?} should not be understood");
        }
    }
}
