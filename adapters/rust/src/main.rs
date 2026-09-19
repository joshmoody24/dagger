//! Reads a Rust snapshot and reports what's defined in it.
//!
//! Two tools, each doing what it's good at. `syn` parses the files, which is how the
//! parts and their spans are worked out. rust-analyzer answers what refers to what,
//! because that's a question about meaning rather than shape, and guessing it from
//! names invents edges that aren't there.
//!
//! rust-analyzer has to be on PATH. Without it there's nothing useful to report: a
//! graph of made-up edges reads worse than no graph.

mod items;
mod modules;

use anyhow::{Context, Result, bail};
use dagger_core::matching::Extraction;
use dagger_core::model::{Locator, Occurrence, Piece, Span};
use dagger_core::reference::{BinderId, Mention, Site, Target};
use dagger_lsp_client::{self as lsp, Lines, Server};
use dagger_protocol::{Note, Request, Response};
use serde::Deserialize;
use serde_json::json;
use std::collections::BTreeMap;
use std::io::{self, Read};
use std::path::Path;

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
        Request::Describe { .. } => Ok(Response::Described {
            include: vec!["**/*.rs".to_string()],
            revisions: None,
        }),
        Request::Extract {
            dir,
            files,
            settings,
            ..
        } => {
            let settings: Settings =
                serde_json::from_value(settings).context("that isn't this adapter's settings")?;
            let (extraction, notes) = extract(Path::new(&dir), &files, &settings)?;
            Ok(Response::Extracted { extraction, notes })
        }
        Request::Materialize { .. } => bail!("this only reads snapshots, it doesn't lay them out"),
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Settings {
    /// Cargo manifests to load besides the one at the root.
    ///
    /// A workspace can leave a crate out — dagger's own window is kept out of its workspace
    /// so a build of the tool doesn't drag a webview in with it — and rust-analyzer then
    /// knows nothing about the files in it. Every definition there comes back with no
    /// contract and no callers, which is worse than slow.
    linked: Vec<String>,
}

struct Parsed {
    path: String,
    lines: Lines,
    found: Vec<items::Found>,
}

fn extract(dir: &Path, files: &[String], settings: &Settings) -> Result<(Extraction, Vec<Note>)> {
    let mut modules = modules::Modules::default();
    let mut notes = Vec::new();

    let parsed: Vec<Parsed> = files
        .iter()
        .filter(|path| path.ends_with(".rs"))
        .filter_map(|path| match parse(dir, path, &mut modules) {
            Ok(parsed) => Some(parsed),
            Err(error) => {
                notes.push(Note {
                    message: format!("skipped it: {error:#}"),
                    file: Some(path.clone()),
                });
                None
            }
        })
        .collect();

    let mut occurrences: Vec<Occurrence> = parsed
        .iter()
        .flat_map(|file| file.found.iter().map(|found| occurrence(file, found)))
        .collect();

    let mentions = bind(dir, &parsed, &mut occurrences, &mut notes, settings)?;
    notes.append(&mut modules.notes);

    Ok((
        Extraction {
            occurrences,
            mentions,
        },
        notes,
    ))
}

/// A file that won't parse is skipped and spoken about, rather than taking the whole
/// snapshot down with it. Half a review beats none.
fn parse(dir: &Path, path: &str, modules: &mut modules::Modules) -> Result<Parsed> {
    let source =
        std::fs::read_to_string(dir.join(path)).with_context(|| format!("couldn't read {path}"))?;
    let file = syn::parse_file(&source).with_context(|| format!("couldn't parse {path}"))?;
    let scope = modules.path_of(dir, path);

    let mut found = items::module(&file.attrs, &file.items, &scope, 0..source.len())
        .into_iter()
        .collect::<Vec<_>>();
    found.extend(items::find(&file.items, &scope));

    Ok(Parsed {
        path: path.to_string(),
        found,
        lines: Lines::new(&source),
    })
}

fn occurrence(file: &Parsed, found: &items::Found) -> Occurrence {
    let parts = found
        .parts
        .iter()
        .map(|(part, ranges)| {
            let pieces = ranges
                .iter()
                .map(|range| Piece {
                    text: file.lines.slice(range).to_string(),
                    span: Span {
                        start: range.start as u32,
                        end: range.end as u32,
                    },
                    file: None,
                })
                .collect();
            (*part, pieces)
        })
        .collect();

    Occurrence {
        locator: locator(found),
        kind: found.kind.to_string(),
        file: file.path.clone(),
        parts,
        contract: None,
    }
}

fn locator(found: &items::Found) -> Locator {
    Locator {
        scope: found.scope.clone(),
        name: found.name.clone(),
    }
}

/// Asks rust-analyzer who refers to each definition, and what each one looks like from
/// outside. One question per definition rather than per name mentioned, which keeps the
/// conversation short enough to be worth having.
fn bind(
    dir: &Path,
    parsed: &[Parsed],
    occurrences: &mut [Occurrence],
    notes: &mut Vec<Note>,
    settings: &Settings,
) -> Result<Vec<Mention>> {
    // Indexing is the slow part by a wide margin, so it's worth admitting to.
    eprintln!("  waiting for rust-analyzer to index");
    let mut options = json!({
        // Nothing here needs macros expanded or build scripts run, and both cost real
        // time on a cold tree.
        "cargo": { "buildScripts": { "enable": false } },
        "procMacro": { "enable": false },
    });
    if !settings.linked.is_empty() {
        options["linkedProjects"] = json!(settings.linked);
    }
    let mut server = Server::start(&["rust-analyzer".to_string()], dir, options)?;
    server.wait_until(|message| {
        message["method"] == "experimental/serverStatus"
            && message["params"]["quiescent"] == serde_json::Value::Bool(true)
    })?;
    let count: usize = parsed.iter().map(|file| file.found.len()).sum();
    eprintln!("  asking rust-analyzer about {count} definitions");
    let root = dir.canonicalize()?;
    let by_path: BTreeMap<&str, &Parsed> = parsed
        .iter()
        .map(|file| (file.path.as_str(), file))
        .collect();

    let mut mentions = Vec::new();
    let mut contracts: BTreeMap<Locator, String> = BTreeMap::new();

    for file in parsed {
        let uri = lsp::uri(&root.join(&file.path));

        for found in file.found.iter().filter(|found| found.referenceable()) {
            let (line, column) = file.lines.position(found.name_at.start);
            let at = json!({
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": column },
            });

            match server.request("textDocument/hover", at.clone()) {
                Ok(hover) => {
                    if let Some(contract) = signature(&hover) {
                        contracts.insert(locator(found), contract);
                    }
                }
                Err(error) => notes.push(Note {
                    message: format!("couldn't ask about {}: {error:#}", found.name),
                    file: Some(file.path.clone()),
                }),
            }

            let mut question = at;
            question["context"] = json!({ "includeDeclaration": false });
            let referrers = match server.request("textDocument/references", question) {
                Ok(referrers) => referrers,
                Err(error) => {
                    notes.push(Note {
                        message: format!("couldn't find what uses {}: {error:#}", found.name),
                        file: Some(file.path.clone()),
                    });
                    continue;
                }
            };

            mentions.extend(referring(&referrers, &root, &by_path, found));
        }
    }

    for occurrence in occurrences.iter_mut() {
        occurrence.contract = contracts.get(&occurrence.locator).cloned();
    }

    Ok(mentions)
}

/// Each place rust-analyzer found, turned into a mention from whichever definition
/// encloses it. A reference from outside any definition we know about is dropped: there
/// is nothing to hang it on.
fn referring(
    referrers: &serde_json::Value,
    root: &Path,
    by_path: &BTreeMap<&str, &Parsed>,
    to: &items::Found,
) -> Vec<Mention> {
    let Some(places) = referrers.as_array() else {
        return Vec::new();
    };

    places
        .iter()
        .filter_map(|place| {
            let path = relative(place["uri"].as_str()?, root)?;
            let file = by_path.get(path.as_str())?;
            let line = place["range"]["start"]["line"].as_u64()? as u32;
            let column = place["range"]["start"]["character"].as_u64()? as u32;
            let at = file.lines.offset(line, column);

            let from = innermost(file, at)?;
            let part = from.part_at(at)?;

            Some(Mention {
                from: locator(from),
                to: Target::Known(locator(to)),
                site: Site {
                    part,
                    span: Span {
                        start: at as u32,
                        end: (at + to.name.len()) as u32,
                    },
                    found_by: BinderId("rust-analyzer".to_string()),
                },
            })
        })
        .collect()
}

fn relative(uri: &str, root: &Path) -> Option<String> {
    let path = uri.strip_prefix("file://")?;
    Some(
        Path::new(path)
            .strip_prefix(root)
            .ok()?
            .to_string_lossy()
            .into_owned(),
    )
}

/// The tightest definition covering a spot, so a method's references land on the method
/// rather than on whatever encloses it.
fn innermost(file: &Parsed, at: usize) -> Option<&items::Found> {
    file.found
        .iter()
        .filter(|found| found.extent().contains(&at))
        .min_by_key(|found| found.extent().len())
}

/// Hover is markdown with the definition fenced off in it, which is rust-analyzer's
/// account of what the thing looks like from outside.
///
/// The first block names the module it lives in, so the one wanted is the last — but only
/// of those before the rule. Past the rule comes the doc comment, and a doc comment's
/// examples are fenced rust too. Reading one of those as the contract would turn editing
/// an example into breaking every caller.
fn signature(hover: &serde_json::Value) -> Option<String> {
    let markdown = hover["contents"]["value"].as_str()?;
    let declaration = markdown.split("\n---").next().unwrap_or(markdown);
    let mut blocks = Vec::new();
    let mut current: Option<Vec<&str>> = None;

    for line in declaration.lines() {
        match (&mut current, line.starts_with("```")) {
            (None, true) if line.starts_with("```rust") => current = Some(Vec::new()),
            (Some(code), true) => {
                blocks.push(code.join("\n"));
                current = None;
            }
            (Some(code), false) => code.push(line),
            _ => {}
        }
    }

    blocks.into_iter().rfind(|block| !block.is_empty())
}
