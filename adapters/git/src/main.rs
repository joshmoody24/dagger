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
#[serde(default)]
struct Settings {
    /// Point the snapshot at the repository's ignored files — build output, installed
    /// packages — instead of leaving them out. On by default: without them, tooling that
    /// reads generated declarations has to compile everything from source instead.
    carry_ignored: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            carry_ignored: true,
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
            let settings: Settings = serde_json::from_value(settings).unwrap_or_default();
            let dir = materialize(&rev, settings.carry_ignored)?;
            let files = Some(listing(&["ls-tree", "-r", "--name-only", "-z", &rev])?);
            Ok(Response::Materialized {
                dir: dir.to_string_lossy().into_owned(),
                temporary: true,
                files,
            })
        }
        Request::Describe { .. } => Ok(Response::Described {
            include: Vec::new(),
            revisions: Some(worth_reviewing()?),
        }),
        Request::Extract { .. } => bail!("git only lays snapshots out, it doesn't read them"),
    }
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

/// Tracked files plus anything new that isn't ignored, which is the same set git
/// status talks about.
fn current_files() -> Result<Vec<String>> {
    listing(&[
        "ls-files",
        "--cached",
        "--others",
        "--exclude-standard",
        "-z",
    ])
}

fn listing(args: &[&str]) -> Result<Vec<String>> {
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

    Ok(String::from_utf8(output.stdout)?
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
