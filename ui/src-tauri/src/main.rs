//! A window around the page that reads a review.
//!
//! It runs `dagger --json` and hands the answer over, and does nothing else. Keeping the
//! reading of a repository in dagger rather than in here means the window can't drift from
//! what the command line says, and that nothing about adapters or language servers has to
//! be repeated on this side.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::io::{BufRead, BufReader, Read};
use tauri::Emitter;
use std::path::PathBuf;
use std::process::{Command, Stdio};

/// Runs a review and gives back what dagger said, verbatim.
///
/// `repo` is the directory to read, which is also where dagger looks for its own
/// configuration. Revisions are left to dagger when the caller doesn't name any: it knows
/// whether there's unfinished work worth looking at.
/// Reading takes the better part of a minute on a cold tree, and a command that isn't
/// asynchronous is run on the thread the window itself is drawn on — so the page couldn't
/// paint, not even the line saying what it was waiting for. The work goes to a thread of
/// its own and the window stays alive while it happens.
#[tauri::command]
async fn review(
    window: tauri::Window,
    repo: String,
    before: Option<String>,
    after: Option<String>,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut dagger = Command::new(found());
        dagger
            .arg("--json")
            .current_dir(&repo)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let (Some(before), Some(after)) = (before, after) {
            dagger.args([before, after]);
        }

        let mut run = dagger
            .spawn()
            .map_err(|error| format!("couldn't run dagger in {repo}: {error}"))?;

        /* Passed along as it arrives rather than kept until the end. A reading takes long
         * enough that a window with nothing on it looks like a window that has stopped, and
         * dagger already says what it's doing — which snapshot, which extractor, how far
         * through. All that was missing was somewhere for it to go. */
        let told = run.stderr.take().map(|stderr| {
            let window = window.clone();
            std::thread::spawn(move || {
                let mut kept = String::new();
                for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                    kept.push_str(&line);
                    kept.push('\n');
                    let _ = window.emit("dagger://said", &line);
                }
                kept
            })
        });

        let mut said = String::new();
        if let Some(mut stdout) = run.stdout.take() {
            stdout
                .read_to_string(&mut said)
                .map_err(|error| format!("dagger said something odd: {error}"))?;
        }

        let ended = run
            .wait()
            .map_err(|error| format!("dagger didn't finish: {error}"))?;
        let wrong = told.and_then(|told| told.join().ok()).unwrap_or_default();

        if !ended.success() {
            return Err(wrong.trim().to_string());
        }
        Ok(said)
    })
    .await
    .map_err(|error| format!("the reading didn't finish: {error}"))?
}

/// Which dagger to run.
///
/// Whatever sits beside this window, when there is one — a window built from this repository
/// should use the tool built alongside it rather than whichever version somebody installed
/// years ago. Failing that, whatever is on PATH.
fn found() -> PathBuf {
    let beside = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join("dagger")));

    match beside {
        Some(path) if path.exists() => path,
        _ => PathBuf::from("dagger"),
    }
}

/// What this window was opened on, as the command line put it.
///
/// Revisions are optional in the same way they're optional on the command line: named, they
/// are what gets read; left out, dagger decides, which means the working changes.
#[derive(serde::Serialize)]
struct Opened {
    repo: String,
    before: Option<String>,
    after: Option<String>,
}

#[tauri::command]
fn opened() -> Opened {
    Opened {
        repo: said("--repo").unwrap_or_else(|| {
            std::env::current_dir()
                .map(|here| here.to_string_lossy().into_owned())
                .unwrap_or_else(|_| ".".to_string())
        }),
        before: said("--before"),
        after: said("--after"),
    }
}

/// What was written after a flag, when it was written at all.
fn said(flag: &str) -> Option<String> {
    let mut args = std::env::args().skip_while(|arg| arg != flag);
    args.next()?;
    args.next()
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![review, opened])
        .run(tauri::generate_context!())
        .expect("the window should open");
}
