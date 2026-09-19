use crate::config::{Adapter, Extractor};
use anyhow::{Context, Result, bail};
use dagger_core::matching::Extraction;
use dagger_protocol::{Note, Request, Response};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// An adapter is a command, read the way a shell would read it: a slash makes it a
/// path in the repo, anything else comes off PATH.
fn resolve(repo: &Path, adapter: &str) -> PathBuf {
    if adapter.contains('/') {
        repo.join(adapter)
    } else {
        PathBuf::from(adapter)
    }
}

fn ask(repo: &Path, adapter: &str, args: &[String], request: &Request) -> Result<Response> {
    let program = resolve(repo, adapter);
    let mut child = Command::new(&program)
        .args(args)
        .current_dir(repo)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .with_context(|| format!("couldn't start {}", program.display()))?;

    let payload = serde_json::to_vec(request)?;
    child
        .stdin
        .take()
        .expect("stdin was piped")
        .write_all(&payload)?;

    let output = child.wait_with_output()?;
    if !output.status.success() {
        bail!("{} exited badly", program.display());
    }

    serde_json::from_slice(&output.stdout)
        .with_context(|| format!("{} said something we couldn't read", program.display()))
}

/// The files an extractor claims when the repo hasn't said. Asking beats guessing
/// from the adapter's name, which would break the moment someone renamed theirs.
pub fn describe(repo: &Path, extractor: &Extractor) -> Result<Vec<String>> {
    match ask(
        repo,
        &extractor.adapter,
        &extractor.args,
        &Request::Describe,
    )? {
        Response::Described { include } => Ok(include),
        Response::Failed { message } => {
            bail!(
                "{} wouldn't say what it reads: {message}",
                extractor.adapter
            )
        }
        other => bail!(
            "asked {} what it reads and got {other:?}",
            extractor.adapter
        ),
    }
}

/// A revision sitting on disk, whether clearing it up is our job, and what the
/// adapter says is in there. No listing means we have to look for ourselves.
pub struct Snapshot {
    pub dir: PathBuf,
    pub temporary: bool,
    pub files: Option<Vec<String>>,
}

pub fn materialize(repo: &Path, snapshots: &Adapter, rev: &str) -> Result<Snapshot> {
    let request = Request::Materialize {
        rev: rev.to_string(),
    };
    match ask(repo, &snapshots.adapter, &snapshots.args, &request)? {
        Response::Materialized {
            dir,
            temporary,
            files,
        } => Ok(Snapshot {
            dir: PathBuf::from(dir),
            temporary,
            files,
        }),
        Response::Failed { message } => bail!("couldn't lay out {rev}: {message}"),
        other => bail!(
            "asked {} for a snapshot and got {other:?}",
            snapshots.adapter
        ),
    }
}

pub fn extract(
    repo: &Path,
    extractor: &Extractor,
    dir: &Path,
    files: &[String],
) -> Result<(Extraction, Vec<Note>)> {
    let request = Request::Extract {
        dir: dir.to_string_lossy().into_owned(),
        files: files.to_vec(),
    };
    match ask(repo, &extractor.adapter, &extractor.args, &request)? {
        Response::Extracted { extraction, notes } => Ok((extraction, notes)),
        Response::Failed { message } => {
            bail!(
                "{} couldn't read the snapshot: {message}",
                extractor.adapter
            )
        }
        other => bail!("asked {} to extract and got {other:?}", extractor.adapter),
    }
}
