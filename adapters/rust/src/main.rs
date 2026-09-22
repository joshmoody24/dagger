//! Reads a Rust snapshot and reports what's defined in it.
//!
//! `syn` finds the definitions and their spans. rust-analyzer (required on PATH) says
//! what refers to what, since guessing that from names invents edges.

mod items;
mod modules;

use anyhow::{Context, Result, bail};
use dagger_core::matching::Extraction;
use dagger_core::model::{Locator, Occurrence, Part, Piece, Role, Span};
use dagger_core::prose::preamble;
use dagger_core::reference::BinderId;
use dagger_lsp_client::walk::{Reach, Source, Walk, Walked};
use dagger_lsp_client::{Lines, Server};
use dagger_protocol::{Changed, Note, Progress, Request, Response};
use serde::Deserialize;
use serde_json::json;
use std::collections::BTreeMap;
use std::io::{self, Read};
use std::ops::Range;
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
        Request::Describe { settings } => Ok(Response::Described {
            // Settings are checked here so a bad one is reported before any snapshot is laid out.
            include: settings_of(settings).map(|_| vec!["**/*.rs".to_string()])?,
            revisions: None,
            usage: Vec::new(),
        }),
        Request::Extract {
            dir,
            files,
            changed,
            ripples,
            settings,
        } => {
            let settings = settings_of(settings)?;
            let (extraction, notes) =
                extract(Path::new(&dir), &files, &changed, ripples, &settings)?;
            Ok(Response::Extracted { extraction, notes })
        }
        Request::Materialize { .. } | Request::Resolve { .. } => {
            bail!("this only reads snapshots, it doesn't lay them out")
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Settings {
    /// Cargo manifests to load besides the root one, for crates the workspace leaves out;
    /// rust-analyzer otherwise knows nothing about their files.
    #[serde(default)]
    linked: Vec<String>,
    /// How many files to chase users of before giving up. A backstop for runaways, not a
    /// setting to reach for: `--ripples` is how far a reading goes.
    #[serde(default = "files_to_walk")]
    max_walk: usize,
}

fn files_to_walk() -> usize {
    Reach::WALK
}

/// Unknown settings are rejected rather than ignored, so a typo doesn't silently change
/// the run.
fn settings_of(settings: serde_json::Value) -> Result<Settings> {
    let settings = if settings.is_null() {
        json!({})
    } else {
        settings
    };
    serde_json::from_value(settings)
        .context("dagger-rust was told something under settings that it doesn't know")
}

struct Parsed {
    path: String,
    lines: Lines,
    found: Vec<items::Found>,
}

fn extract(
    dir: &Path,
    files: &[String],
    changed: &[Changed],
    ripples: u32,
    settings: &Settings,
) -> Result<(Extraction, Vec<Note>)> {
    eprintln!(
        "{}",
        Progress::StartingServer {
            name: "rust-analyzer".to_string()
        }
    );
    let mut options = json!({
        // Neither is needed here, and both cost real time on a cold tree.
        "cargo": { "buildScripts": { "enable": false } },
        "procMacro": { "enable": false },
    });
    if !settings.linked.is_empty() {
        options["linkedProjects"] = json!(settings.linked);
    }
    let mut server = Server::start(&["rust-analyzer".to_string()], dir, options)?;
    eprintln!(
        "{}",
        Progress::Indexing {
            name: "rust-analyzer".to_string()
        }
    );
    server.wait_until(|message| {
        message["method"] == "experimental/serverStatus"
            && message["params"]["quiescent"] == serde_json::Value::Bool(true)
    })?;

    let root = dir.canonicalize()?;
    let ours: std::collections::BTreeSet<String> = files
        .iter()
        .filter(|path| path.ends_with(".rs"))
        .cloned()
        .collect();

    let mut walk = Walk::new(
        server,
        root,
        RustSource::default(),
        Reach {
            binder: BinderId("rust-analyzer".to_string()),
            ours: ours.clone(),
            ripples,
            walk_limit: settings.max_walk,
            open_limit: Reach::OPEN,
        },
    );

    // Parsing with `syn` is cheap, so every file is parsed up front; only the
    // rust-analyzer side stays lazy.
    for path in &ours {
        walk.look(path);
    }
    walk.spread(changed);

    let Walked {
        source,
        seen,
        mentions,
        contracts,
        mut notes,
    } = walk.finish();
    let parsed: BTreeMap<String, Parsed> = seen
        .into_iter()
        .map(|(path, (lines, found))| (path.clone(), Parsed { path, lines, found }))
        .collect();

    let mut occurrences: Vec<Occurrence> = parsed
        .values()
        .flat_map(|file| file.found.iter().map(|found| occurrence(file, found)))
        .collect();
    for occurrence in occurrences.iter_mut() {
        occurrence.contract = contracts.get(&occurrence.locator).cloned();
    }

    notes.extend(source.modules.notes);
    eprintln!(
        "{}",
        Progress::Finished {
            files: parsed.len(),
            definitions: occurrences.len(),
        }
    );

    Ok((
        Extraction {
            occurrences,
            mentions,
        },
        notes,
    ))
}

/// Discovers a Rust file's definitions by parsing it, and reads a contract back out of
/// what rust-analyzer says on hover.
#[derive(Default)]
struct RustSource {
    modules: modules::Modules,
}

impl Source for RustSource {
    type Item = items::Found;

    fn open(
        &mut self,
        _server: &mut Server,
        dir: &Path,
        path: &str,
    ) -> Result<(Lines, Vec<items::Found>)> {
        let parsed = parse(dir, path, &mut self.modules)?;
        Ok((parsed.lines, parsed.found))
    }

    fn contract(&self, hover: &serde_json::Value) -> Option<String> {
        signature(hover)
    }
}

/// A file that won't parse is skipped and noted rather than failing the whole snapshot.
fn parse(dir: &Path, path: &str, modules: &mut modules::Modules) -> Result<Parsed> {
    let source =
        std::fs::read_to_string(dir.join(path)).with_context(|| format!("couldn't read {path}"))?;
    let file = syn::parse_file(&source).with_context(|| format!("couldn't parse {path}"))?;
    let scope = modules.path_of(dir, path);

    let mut found = items::module(&file.attrs, &file.items, &scope, 0..source.len(), false)
        .into_iter()
        .collect::<Vec<_>>();
    found.extend(items::find(&file.items, &scope));
    told(&source, &mut found);

    Ok(Parsed {
        path: path.to_string(),
        found,
        lines: Lines::new(&source),
    })
}

/// Gives each definition the plain comment written above it. `syn` keeps `///` but the
/// lexer drops `//` and `/* */`, so without this a change to such a comment belongs to
/// no definition.
fn told(source: &str, found: &mut [items::Found]) {
    let claimed: Vec<Range<usize>> = found
        .iter()
        .flat_map(|one| one.parts.values().flatten().cloned())
        .collect();

    for one in found.iter_mut() {
        let spans = one.parts.values().flatten();
        let (Some(from), Some(to)) = (
            spans.clone().map(|range| range.start).min(),
            spans.map(|range| range.end).max(),
        ) else {
            continue;
        };
        // Nothing sits above the file's own module; looking would steal the first item's prose.
        if from == 0 {
            continue;
        }

        if let Some(prose) = preamble(source, &(from..to), &claimed) {
            let docs = one.parts.entry(Part::Docs).or_default();
            docs.push(prose);
            docs.sort_by_key(|range| range.start);
        }
    }
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
                    line: file.lines.position(range.start).0 + 1,
                    file: file.path.clone(),
                })
                .collect();
            (*part, pieces)
        })
        .collect();

    Occurrence {
        locator: locator(found),
        role: role_of(found.kind),
        parent: holding(file, found).map(locator),
        kind: found.kind.to_string(),
        file: file.path.clone(),
        parts,
        contract: None,
    }
}

/// Only the extractor knows which kinds hold others; nothing downstream knows Rust has
/// `impl` blocks.
fn role_of(kind: &str) -> Role {
    match kind {
        "module" | "impl" | "trait" => Role::Container,
        _ => Role::Item,
    }
}

/// The smallest definition that covers this one. Read off spans rather than scope names:
/// a method's scope names its type, not the `impl` block it sits in.
fn holding<'a>(file: &'a Parsed, found: &items::Found) -> Option<&'a items::Found> {
    file.found
        .iter()
        .filter(|other| other.covers != found.covers)
        .filter(|other| {
            other.covers.start <= found.covers.start && other.covers.end >= found.covers.end
        })
        .min_by_key(|other| other.covers.end - other.covers.start)
}

fn locator(found: &items::Found) -> Locator {
    Locator {
        scope: found.scope.clone(),
        name: found.name.clone(),
    }
}

/// The contract is the last fenced rust block before the `---` rule: the first block only
/// names the module, and past the rule doc examples are fenced rust too, so reading one
/// would make editing an example a breaking change.
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
