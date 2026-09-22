//! `dagger open`: serves the page to a browser and answers its request for a review by
//! running dagger itself, the same way ui/vite.config.js does while developing the page.

use anyhow::{Context, Result};
use include_dir::{Dir, include_dir};
use std::io::{BufRead, BufReader, Read};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc::{Receiver, channel};
use tiny_http::{Header, Request, Response, Server, StatusCode};

static PAGE: Dir = include_dir!("$CARGO_MANIFEST_DIR/../../ui/dist");

pub fn serve(repo: &Path, asked: Vec<String>) -> Result<()> {
    let server = Server::http("127.0.0.1:0").map_err(|error| anyhow::anyhow!("{error}"))?;
    let url = format!("http://{}/", server.server_addr());
    eprintln!("{url}");
    if let Err(error) = open::that(&url) {
        eprintln!("couldn't open a browser: {error}");
    }

    let repo = repo.to_path_buf();
    for request in server.incoming_requests() {
        let (repo, asked) = (repo.clone(), asked.clone());
        std::thread::spawn(move || {
            if let Err(error) = answer(request, &repo, &asked) {
                eprintln!("couldn't answer: {error:#}");
            }
        });
    }
    Ok(())
}

fn answer(request: Request, repo: &Path, asked: &[String]) -> Result<()> {
    let url = request.url().to_string();
    let (path, query) = url.split_once('?').unwrap_or((&url, ""));
    match path {
        "/review" => {
            let named: Vec<String> = query
                .split('&')
                .filter_map(|pair| pair.split_once('='))
                .filter(|(key, _)| *key == "read")
                .map(|(_, value)| decoded(value))
                .collect();
            let reading = if named.is_empty() { asked } else { &named };
            Ok(request.respond(review(repo, reading)?)?)
        }
        path => Ok(request.respond(file(path))?),
    }
}

/// A query value as the browser wrote it.
fn decoded(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        match bytes[at] {
            b'+' => out.push(b' '),
            b'%' if at + 2 < bytes.len() => match u8::from_str_radix(&value[at + 1..at + 3], 16) {
                Ok(byte) => {
                    out.push(byte);
                    at += 2;
                }
                Err(_) => out.push(b'%'),
            },
            byte => out.push(byte),
        }
        at += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn header(name: &str, value: &str) -> Header {
    Header::from_bytes(name, value).expect("a header we wrote ourselves")
}

/// A file out of the built page, by the same name a browser asks for it.
fn file(path: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    let name = match path.trim_start_matches('/') {
        "" => "index.html",
        name => name,
    };
    match PAGE.get_file(name) {
        Some(found) => Response::from_data(found.contents())
            .with_header(header("content-type", content_type(name))),
        None => Response::from_string("not here").with_status_code(404),
    }
}

fn content_type(name: &str) -> &'static str {
    match name.rsplit('.').next() {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "text/javascript",
        Some("css") => "text/css",
        Some("json") => "application/json",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("woff2") => "font/woff2",
        Some("wasm") => "application/wasm",
        _ => "application/octet-stream",
    }
}

/// Newline-delimited JSON, sent as it happens: each stderr line as a note, then the
/// review itself the moment dagger's one stdout line ends, since it still has snapshots
/// to remove afterwards. Nothing on stdout by the time it exits means it went wrong.
fn review(repo: &Path, reading: &[String]) -> Result<Response<Lines>> {
    let mut child = Command::new(std::env::current_exe()?)
        .arg("--json")
        .args(reading)
        .current_dir(repo)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("couldn't run dagger")?;
    let stdout = child.stdout.take().expect("stdout was piped");
    let stderr = child.stderr.take().expect("stderr was piped");

    let (send, lines) = channel::<Said>();

    let noted = std::thread::spawn({
        let send = send.clone();
        move || {
            BufReader::new(stderr)
                .lines()
                .map_while(Result::ok)
                .inspect(|line| {
                    eprintln!("{line}");
                    let note = serde_json::json!({ "note": line }).to_string();
                    let _ = send.send(Said::Note(note));
                })
                .fold(String::new(), |said, line| said + &line + "\n")
        }
    });

    std::thread::spawn({
        let send = send.clone();
        move || {
            let mut line = String::new();
            let _ = BufReader::new(stdout).read_line(&mut line);
            match line.trim_end_matches('\n') {
                "" => {}
                review => {
                    let _ = send.send(Said::Last(format!("{{\"review\":{review}}}")));
                }
            }
        }
    });

    std::thread::spawn(move || {
        let status = child.wait();
        let said = noted.join().unwrap_or_default();
        let why = match said.trim() {
            "" => format!(
                "dagger gave up with {}",
                status.map_or("no exit status".to_string(), |code| code.to_string())
            ),
            wrong => wrong.to_string(),
        };
        let _ = send.send(Said::Last(serde_json::json!({ "wrong": why }).to_string()));
    });

    let headers = vec![
        header("content-type", "application/x-ndjson"),
        header("cache-control", "no-store"),
    ];
    Ok(Response::new(
        StatusCode(200),
        headers,
        Lines {
            from: lines,
            left: Vec::new(),
            done: false,
        },
        None,
        None,
    ))
}

enum Said {
    Note(String),
    /// The review or the failure. Whichever comes first ends the stream, so the other
    /// finds nobody listening.
    Last(String),
}

/// A body read a line at a time as the lines are said.
struct Lines {
    from: Receiver<Said>,
    left: Vec<u8>,
    done: bool,
}

impl Read for Lines {
    fn read(&mut self, into: &mut [u8]) -> std::io::Result<usize> {
        if self.left.is_empty() {
            if self.done {
                return Ok(0);
            }
            let line = match self.from.recv() {
                Ok(Said::Note(line)) => line,
                Ok(Said::Last(line)) => {
                    self.done = true;
                    line
                }
                Err(_) => return Ok(0),
            };
            self.left = format!("{line}\n").into_bytes();
        }
        let taking = into.len().min(self.left.len());
        into[..taking].copy_from_slice(&self.left[..taking]);
        self.left.drain(..taking);
        Ok(taking)
    }
}
