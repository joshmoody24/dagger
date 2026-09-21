use crate::config::{Adapter, Extractor};
use anyhow::{Context, Result, bail};
use dagger_core::matching::Extraction;
use dagger_protocol::{Note, Request, Response, Revisions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Settings travel as JSON, since that's what the wire speaks, but a repo writes them
/// as TOML.
fn json(settings: &toml::Value) -> serde_json::Value {
    serde_json::to_value(settings).unwrap_or(serde_json::Value::Null)
}

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

/// What an adapter says about itself. Asking beats guessing from its name, which would
/// break the moment someone renamed theirs.
pub fn describe(
    repo: &Path,
    adapter: &str,
    args: &[String],
    settings: &toml::Value,
) -> Result<Described> {
    match ask(
        repo,
        adapter,
        args,
        &Request::Describe {
            settings: json(settings),
        },
    )? {
        Response::Described { include, revisions } => Ok(Described { include, revisions }),
        Response::Failed { message } => bail!("{adapter} wouldn't say what it does: {message}"),
        other => bail!("asked {adapter} what it does and got {other:?}"),
    }
}

pub struct Described {
    pub include: Vec<String>,
    /// Only a snapshot adapter fills this in.
    pub revisions: Option<Revisions>,
}

/// A revision sitting on disk, whether clearing it up is our job, and what the
/// adapter says is in there. No listing means we have to look for ourselves.
pub struct Snapshot {
    pub dir: PathBuf,
    pub temporary: bool,
    pub files: Option<Vec<String>>,
}

/// A laid-out snapshot takes itself away.
///
/// Tidying up by hand means every path out of the reading has to remember to do it, and the
/// ones that don't are the paths nobody walks on purpose: the second snapshot failing to
/// lay out leaves the first sitting in the temporary directory, and an error anywhere after
/// leaves both. A copy of a repository is not a small thing to leave behind.
impl Drop for Snapshot {
    fn drop(&mut self) {
        if self.temporary {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }
}

pub fn materialize(repo: &Path, snapshots: &Adapter, rev: &str) -> Result<Snapshot> {
    let request = Request::Materialize {
        rev: rev.to_string(),
        settings: json(&snapshots.settings),
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
    changed: &[String],
) -> Result<(Extraction, Vec<Note>)> {
    let request = Request::Extract {
        dir: dir.to_string_lossy().into_owned(),
        files: files.to_vec(),
        changed: changed.to_vec(),
        settings: json(&extractor.settings),
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
