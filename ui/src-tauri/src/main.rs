//! A window around the page that reads a review.
//!
//! It runs `dagger --json` and hands the answer over, and does nothing else. Keeping the
//! reading of a repository in dagger rather than in here means the window can't drift from
//! what the command line says, and that nothing about adapters or language servers has to
//! be repeated on this side.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::process::Command;

/// Runs a review and gives back what dagger said, verbatim.
///
/// `repo` is the directory to read, which is also where dagger looks for its own
/// configuration. Revisions are left to dagger when the caller doesn't name any: it knows
/// whether there's unfinished work worth looking at.
#[tauri::command]
fn review(repo: String, before: Option<String>, after: Option<String>) -> Result<String, String> {
    let mut dagger = Command::new(found());
    dagger.arg("--json").current_dir(&repo);
    if let (Some(before), Some(after)) = (before, after) {
        dagger.args([before, after]);
    }

    let run = dagger
        .output()
        .map_err(|error| format!("couldn't run dagger in {repo}: {error}"))?;

    if !run.status.success() {
        return Err(String::from_utf8_lossy(&run.stderr).trim().to_string());
    }
    String::from_utf8(run.stdout).map_err(|error| format!("dagger said something odd: {error}"))
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

/// Which repository this window was opened on: `--repo`, or wherever it was started from.
#[tauri::command]
fn repo() -> String {
    let mut args = std::env::args().skip_while(|arg| arg != "--repo");
    args.next();

    args.next()
        .unwrap_or_else(|| {
            std::env::current_dir()
                .map(|here| here.to_string_lossy().into_owned())
                .unwrap_or_else(|_| ".".to_string())
        })
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![review, repo])
        .run(tauri::generate_context!())
        .expect("the window should open");
}
