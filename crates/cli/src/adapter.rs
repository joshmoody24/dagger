use crate::config::{Adapter, Extractor};
use anyhow::{Context, Result, bail};
use dagger_core::matching::Extraction;
use dagger_protocol::{Changed, Described, Note, Request, Response, Revisions};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Settings are written as TOML but sent to adapters as JSON.
fn json(settings: &toml::Value) -> serde_json::Value {
    serde_json::to_value(settings).unwrap_or(serde_json::Value::Null)
}

pub const BUILT_IN: [&str; 3] = ["git", "lsp", "rust"];

/// A built-in name runs this same binary as that subcommand. Otherwise shell rules: a
/// slash makes it a path in the repo, anything else comes off PATH.
fn resolve(repo: &Path, adapter: &str) -> Result<(PathBuf, Vec<String>)> {
    if BUILT_IN.contains(&adapter) {
        return Ok((std::env::current_exe()?, vec![adapter.to_string()]));
    }
    let program = if adapter.contains('/') {
        repo.join(adapter)
    } else {
        PathBuf::from(adapter)
    };
    Ok((program, Vec::new()))
}

/// Asks an adapter one thing and waits for the answer. `saying` prefixes the adapter's
/// stderr lines so several adapters sharing a terminal can be told apart; unnamed,
/// stderr goes straight through.
fn ask(
    repo: &Path,
    adapter: &str,
    args: &[String],
    request: &Request,
    saying: Option<&str>,
) -> Result<Response> {
    let (program, first) = resolve(repo, adapter)?;
    let mut child = Command::new(&program)
        .args(first)
        .args(args)
        .current_dir(repo)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(match saying {
            Some(_) => Stdio::piped(),
            None => Stdio::inherit(),
        })
        .spawn()
        .with_context(|| format!("couldn't start {}", program.display()))?;

    let payload = serde_json::to_vec(request)?;
    child
        .stdin
        .take()
        .expect("stdin was piped")
        .write_all(&payload)?;

    // Relayed as it arrives, since progress output is there to fill the wait.
    let output = match saying {
        Some(name) => {
            let said = child.stderr.take().expect("stderr was piped");
            std::thread::scope(|threads| {
                threads.spawn(|| relay(said, name));
                child.wait_with_output()
            })?
        }
        None => child.wait_with_output()?,
    };
    if !output.status.success() {
        bail!("{} exited badly", program.display());
    }

    serde_json::from_slice(&output.stdout)
        .with_context(|| format!("{} said something we couldn't read", program.display()))
}

/// Forwards an adapter's stderr, one prefixed line at a time.
fn relay(said: std::process::ChildStderr, name: &str) {
    for line in std::io::BufReader::new(said).lines().map_while(Result::ok) {
        // One write per line, so two relays can't interleave mid-line.
        eprintln!("{name} · {line}");
    }
}

/// Asks an adapter what it does, rather than guessing from its name.
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
        None,
    )? {
        Response::Described(described) => Ok(described),
        Response::Failed { message } => bail!("{adapter} wouldn't say what it does: {message}"),
        other => bail!("asked {adapter} what it does and got {other:?}"),
    }
}

/// A revision on disk, whether we clean it up, and the adapter's file listing.
/// No listing means we walk the directory ourselves.
pub struct Snapshot {
    pub dir: PathBuf,
    pub temporary: bool,
    pub files: Option<Vec<String>>,
}

/// Cleaned up on drop so every error path removes the copy, including the second
/// snapshot failing to lay out after the first succeeded.
impl Drop for Snapshot {
    fn drop(&mut self) {
        if self.temporary {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }
}

/// Turns the command-line words into two revisions via the snapshot adapter.
pub fn revisions(repo: &Path, snapshots: &Adapter, asked: &[String]) -> Result<Revisions> {
    let request = Request::Resolve {
        asked: asked.to_vec(),
        settings: json(&snapshots.settings),
    };
    match ask(repo, &snapshots.adapter, &snapshots.args, &request, None)? {
        Response::Resolved { revisions } => Ok(revisions),
        // The adapter's wording is passed through as is.
        Response::Failed { message } => bail!("{message}"),
        other => bail!("asked {} what to read and got {other:?}", snapshots.adapter),
    }
}

pub fn materialize(repo: &Path, snapshots: &Adapter, rev: &str) -> Result<Snapshot> {
    let request = Request::Materialize {
        rev: rev.to_string(),
        settings: json(&snapshots.settings),
    };
    match ask(repo, &snapshots.adapter, &snapshots.args, &request, None)? {
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
    changed: &[Changed],
    ripples: u32,
    saying: &str,
) -> Result<(Extraction, Vec<Note>)> {
    let request = Request::Extract {
        dir: dir.to_string_lossy().into_owned(),
        files: files.to_vec(),
        changed: changed.to_vec(),
        ripples,
        settings: json(&extractor.settings),
    };
    match ask(
        repo,
        &extractor.adapter,
        &extractor.args,
        &request,
        Some(saying),
    )? {
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
