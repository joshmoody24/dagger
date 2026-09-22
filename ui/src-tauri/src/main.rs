//! The window around the review page. It runs `dagger --json` and hands the answer over,
//! so nothing about adapters or language servers is repeated on this side.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::io::{BufRead, BufReader, Read};
use tauri::Emitter;
use std::path::PathBuf;
use std::process::{Command, Stdio};

/// Runs a review in `repo` and returns dagger's JSON verbatim. Done on a blocking thread,
/// since a reading can take a minute and would otherwise block the thread the window
/// paints on.
#[tauri::command]
async fn review(
    window: tauri::Window,
    repo: String,
    asked: Vec<String>,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut dagger = Command::new(found());
        dagger
            .arg("--json")
            .current_dir(&repo)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        // Passed on as typed; what the words mean is the snapshot adapter's to say.
        dagger.args(&asked);

        let mut run = dagger
            .spawn()
            .map_err(|error| format!("couldn't run dagger in {repo}: {error}"))?;

        // Streamed as it arrives, so the window has something to show during a long reading.
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

/// Prefers the dagger built beside this window over whatever is on PATH.
fn found() -> PathBuf {
    let beside = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join("dagger")));

    match beside {
        Some(path) if path.exists() => path,
        _ => PathBuf::from("dagger"),
    }
}

/// What this window was opened on. Everything after `--read` goes to dagger untouched.
#[derive(serde::Serialize)]
struct Opened {
    repo: String,
    asked: Vec<String>,
}

#[tauri::command]
fn opened() -> Opened {
    Opened {
        repo: said("--repo").unwrap_or_else(|| {
            std::env::current_dir()
                .map(|here| here.to_string_lossy().into_owned())
                .unwrap_or_else(|_| ".".to_string())
        }),
        asked: rest("--read"),
    }
}

/// Everything after a flag; these are dagger's arguments, not ours.
fn rest(flag: &str) -> Vec<String> {
    let mut args = std::env::args().skip_while(|arg| arg != flag);
    match args.next() {
        Some(_) => args.collect(),
        None => Vec::new(),
    }
}

/// The value after a flag, if the flag was given.
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
