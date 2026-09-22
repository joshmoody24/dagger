//! Lays a git revision out on disk for the extractors to read.
//!
//! Uses `git archive` rather than a checkout so the working tree and index are never
//! touched. The revision `current` is the working tree itself, so it needs no copying.

use anyhow::{Context, Result, bail};
use dagger_protocol::{Described, Request, Response, Snapshots};
use serde::Deserialize;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// What a repo can tell this adapter.
#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Settings {
    /// Whether to link the repository's ignored files (build output, installed packages)
    /// into the snapshot. On by default: without them a language server compiles
    /// everything from source.
    carry_ignored: bool,
    /// The branch others are cut from. Empty means whatever the remote's HEAD is.
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

pub fn answer(request: Request) -> Result<Response> {
    match request {
        Request::Materialize { snapshot, .. } if snapshot == CURRENT => {
            Ok(Response::Materialized {
                dir: std::env::current_dir()?
                    .canonicalize()?
                    .to_string_lossy()
                    .into_owned(),
                temporary: false,
                files: Some(current_files()?),
            })
        }
        Request::Materialize { snapshot, settings } => {
            let settings = settings_of(settings)?;
            let dir = materialize(&snapshot, settings.carry_ignored)?;
            let files = Some(listing(&["ls-tree", "-r", "--name-only", "-z", &snapshot])?);
            Ok(Response::Materialized {
                dir: dir.to_string_lossy().into_owned(),
                temporary: true,
                files,
            })
        }
        Request::Describe { settings } => Ok(Response::Described(Described {
            include: {
                // Settings are checked here so a bad one is reported before any snapshot is laid out.
                settings_of(settings)?;
                Vec::new()
            },
            snapshots: Some(worth_reviewing()?),
            usage: UNDERSTOOD.lines().map(str::to_string).collect(),
        })),
        Request::Resolve { asked, settings } => {
            let settings = settings_of(settings)?;
            Ok(Response::Resolved {
                snapshots: resolve(&asked, &settings)?,
            })
        }
        Request::Extract { .. } => bail!("git only lays snapshots out, it doesn't read them"),
    }
}

/// Unknown settings are rejected rather than ignored, so a typo in `dagger.toml` doesn't
/// silently change the run.
fn settings_of(settings: serde_json::Value) -> Result<Settings> {
    dagger_protocol::settings(
        settings,
        "dagger-git was told something under settings that it doesn't know",
    )
}

/// Not a revision git knows about, so we answer it ourselves.
const CURRENT: &str = "current";

/// Unfinished work is what someone is most likely to want to look at, so that wins
/// when there is any. Failing that, the last thing they committed.
fn worth_reviewing() -> Result<Snapshots> {
    let dirty = !listing(&["status", "--porcelain", "-z"])?.is_empty();
    Ok(if dirty {
        // `current` isn't a commit, so there's no subject to read.
        Snapshots {
            before: "HEAD".to_string(),
            after: CURRENT.to_string(),
            title: None,
        }
    } else {
        Snapshots {
            before: "HEAD~1".to_string(),
            after: "HEAD".to_string(),
            title: subject("HEAD"),
        }
    })
}

/// The two ends of what someone asked for, before git has been asked to resolve them.
#[derive(Debug, PartialEq)]
struct Ends<'a> {
    left: &'a str,
    right: &'a str,
    /// Whether the left end means the merge base (git's `...`) rather than the revision itself.
    parted: bool,
}

/// The trunk, spelled so `resolve` knows to go and find it.
const TRUNK: &str = "";

/// Every way a change can be named. Shown in dagger's help and in the error for anything else.
const UNDERSTOOD: &str = "\
branch <name>        that branch, since it left the trunk
commits <a> <b>      those two revisions
commits <a>..<b>     or <a>...<b>, as git writes them";

/// A bare name is refused rather than guessed: it could mean the branch to read or the
/// branch it came from, and a review of the wrong change looks just like the right one.
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

/// Both ends come back as commits so nothing downstream resolves a name a second time,
/// possibly differently.
fn resolve(asked: &[String], settings: &Settings) -> Result<Snapshots> {
    let Ends {
        left,
        right,
        parted,
    } = read(asked)?;

    let base = if left == TRUNK {
        trunk(settings)?
    } else {
        left.to_string()
    };
    let (base_commit, tip) = (commit(&base)?, commit(right)?);

    let before = if parted {
        let found = say(&["merge-base", &base_commit, &tip])?;
        if found.is_empty() {
            bail!("{right} and {base} share no history, so there's nothing between them");
        }
        if found == tip {
            bail!("{right} has nothing beyond {base}, so there's nothing to read");
        }
        found
    } else {
        base_commit
    };

    Ok(Snapshots {
        before,
        title: subject(&tip),
        after: tip,
    })
}

/// The commit's subject line, if any. `current` isn't a commit, so it has none.
fn subject(commit: &str) -> Option<String> {
    say(&["log", "-1", "--format=%s", commit])
        .ok()
        .filter(|line| !line.is_empty())
}

/// The branch others are cut from: the remote's default branch when git has recorded it,
/// otherwise the usual names.
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

/// A fetched but never checked-out branch is only a remote-tracking ref, which `rev-parse`
/// refuses. Like `git checkout`, try the name as written, then the one remote that has it.
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

    // `--cached` still lists files deleted from disk but not staged, so drop those or the
    // snapshot claims a file that isn't there.
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

fn materialize(snapshot: &str, carry_ignored: bool) -> Result<PathBuf> {
    let commit = rev_parse(snapshot)?;
    let dir = std::env::temp_dir().join(format!("dagger-{}-{}", std::process::id(), commit));
    fs::create_dir_all(&dir).with_context(|| format!("couldn't make {}", dir.display()))?;
    // Canonical, so it matches the paths a language server reports: on macOS the temp
    // directory is behind a symlink, and a prefix check against the other spelling drops
    // every file.
    let dir = dir.canonicalize()?;
    export(&commit, &dir)?;
    if carry_ignored {
        carry(&dir)?;
    }
    Ok(dir)
}

/// Links the repository's ignored files into the snapshot so a language server can read
/// generated declarations instead of compiling every package from source. Linked, not
/// copied, so they reflect the last build rather than this revision; reviewed files aren't affected.
fn carry(dir: &Path) -> Result<()> {
    let repo = std::env::current_dir()?.canonicalize()?;

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
        carried(
            &Carry {
                repo: repo.clone(),
                snapshot: dir.to_path_buf(),
                root: target.clone(),
            },
            &target,
            &link,
            DEEP,
        )
        .with_context(|| format!("couldn't point {path} at the real one"))?;
    }

    Ok(())
}

/// How deep to look for links back into the repository. Two is as deep as package managers
/// keep installed links: a package, or a package in a scope.
const DEEP: u32 = 2;

/// The three roots a carried link is judged against.
struct Carry {
    repo: PathBuf,
    snapshot: PathBuf,
    /// The ignored directory being carried. A link that stays inside it is left alone.
    root: PathBuf,
}

/// A link leading back into the repository is pointed into the snapshot instead: otherwise
/// the language server sees the live copy and the snapshot's as two files and loses every
/// cross-package reference. A directory holding such a link is carried an entry at a time.
fn carried(carry: &Carry, real: &Path, ours: &Path, deep: u32) -> Result<()> {
    if let Some(into) = leads_home(carry, real) {
        return Ok(std::os::unix::fs::symlink(into, ours)?);
    }
    if deep > 0 && real.is_dir() && !real.is_symlink() && holds_a_way_home(carry, real, deep)? {
        fs::create_dir_all(ours)?;
        for entry in fs::read_dir(real)? {
            let name = entry?.file_name();
            carried(carry, &real.join(&name), &ours.join(&name), deep - 1)?;
        }
        return Ok(());
    }
    Ok(std::os::unix::fs::symlink(real, ours)?)
}

/// Where a link leads within the snapshot, when it leads back into the repository.
fn leads_home(carry: &Carry, real: &Path) -> Option<PathBuf> {
    let to = fs::read_link(real).ok()?;
    let to = real.parent()?.join(to).canonicalize().ok()?;
    let inside = to.starts_with(&carry.repo) && !to.starts_with(&carry.root);
    let within = to.strip_prefix(&carry.repo).ok()?;
    inside.then(|| carry.snapshot.join(within))
}

fn holds_a_way_home(carry: &Carry, dir: &Path, deep: u32) -> Result<bool> {
    for entry in fs::read_dir(dir)? {
        let real = entry?.path();
        if leads_home(carry, &real).is_some() {
            return Ok(true);
        }
        if deep > 1
            && real.is_dir()
            && !real.is_symlink()
            && holds_a_way_home(carry, &real, deep - 1)?
        {
            return Ok(true);
        }
    }
    Ok(false)
}

fn rev_parse(snapshot: &str) -> Result<String> {
    say(&["rev-parse", snapshot])
        .with_context(|| format!("git doesn't know the revision {snapshot}"))
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

    #[test]
    fn a_link_back_into_the_repository_is_pointed_at_the_snapshots_copy() {
        let root = std::env::temp_dir().join(format!("dagger-carry-{}", std::process::id()));
        let (repo, snap) = (root.join("repo"), root.join("snap"));
        let installed = repo.join("deps");
        fs::create_dir_all(repo.join("pkgs/a")).unwrap();
        fs::create_dir_all(installed.join("scope")).unwrap();
        fs::create_dir_all(installed.join("b")).unwrap();
        fs::create_dir_all(repo.join("cache/x")).unwrap();
        std::os::unix::fs::symlink("../../pkgs/a", installed.join("scope/a")).unwrap();
        std::os::unix::fs::symlink("b", installed.join("c")).unwrap();
        let repo = repo.canonicalize().unwrap();

        for name in ["deps", "cache"] {
            let carry = Carry {
                repo: repo.clone(),
                snapshot: snap.clone(),
                root: repo.join(name),
            };
            carried(&carry, &repo.join(name), &snap.join(name), DEEP).unwrap();
        }

        let led = |path: &str| fs::read_link(snap.join(path)).unwrap();
        assert_eq!(
            led("deps/scope/a"),
            snap.join("pkgs/a"),
            "into the snapshot"
        );
        assert_eq!(led("deps/b"), repo.join("deps/b"), "at what's installed");
        assert_eq!(
            led("deps/c"),
            repo.join("deps/c"),
            "a link that stays inside is left alone"
        );
        assert_eq!(
            led("cache"),
            repo.join("cache"),
            "nothing leading home, so carried whole"
        );
        fs::remove_dir_all(&root).unwrap();
    }
}
