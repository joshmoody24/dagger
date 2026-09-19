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
use dagger_protocol::{Request, Response};
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

fn answer(request: Request) -> Result<Response> {
    match request {
        Request::Materialize { rev } if rev == CURRENT => Ok(Response::Materialized {
            dir: std::env::current_dir()?.to_string_lossy().into_owned(),
            temporary: false,
            files: Some(current_files()?),
        }),
        Request::Materialize { rev } => {
            let dir = materialize(&rev)?;
            let files = Some(listing(&["ls-tree", "-r", "--name-only", "-z", &rev])?);
            Ok(Response::Materialized {
                dir: dir.to_string_lossy().into_owned(),
                temporary: true,
                files,
            })
        }
        Request::Describe => Ok(Response::Described {
            include: Vec::new(),
        }),
        Request::Extract { .. } => bail!("git only lays snapshots out, it doesn't read them"),
    }
}

/// Not a revision git knows about, so we answer it ourselves.
const CURRENT: &str = "current";

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

fn materialize(rev: &str) -> Result<PathBuf> {
    let commit = rev_parse(rev)?;
    let dir = std::env::temp_dir().join(format!("dagger-{}-{}", std::process::id(), commit));
    fs::create_dir_all(&dir).with_context(|| format!("couldn't make {}", dir.display()))?;
    export(&commit, &dir)?;
    Ok(dir)
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
