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
use dagger_core::model::{Locator, Occurrence, Part, Piece, Role, Span};
use dagger_core::prose::preamble;
use dagger_core::reference::BinderId;
use dagger_lsp_client::walk::{Source, Walk};
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
            // Nothing here needs them, but this is the first thing dagger asks, and a
            // setting nobody understands is worth hearing about before a snapshot has been
            // laid out rather than after.
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
    /// Cargo manifests to load besides the one at the root.
    ///
    /// A workspace can leave a crate out — dagger's own window is kept out of its workspace
    /// so a build of the tool doesn't drag a webview in with it — and rust-analyzer then
    /// knows nothing about the files in it. Every definition there comes back with no
    /// contract and no callers, which is worse than slow.
    #[serde(default)]
    linked: Vec<String>,
    /// How many files to chase users of before giving up and saying so.
    ///
    /// A backstop, not a setting anybody should have to reach for: how far a reading goes
    /// is `--ripples`, and someone who asks for ten steps and gets six because of a number
    /// they never chose has been told something untrue about their own change. So this
    /// sits high enough to catch a runaway and nothing else.
    #[serde(default = "files_to_walk")]
    max_walk: usize,
}

fn files_to_walk() -> usize {
    100_000
}

/// What the repository told this adapter, refused if it isn't something this adapter
/// knows. A setting quietly ignored is worse than one rejected: the run carries on and
/// answers a question nobody asked.
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
        // Nothing here needs macros expanded or build scripts run, and both cost real
        // time on a cold tree.
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
        BinderId("rust-analyzer".to_string()),
        ours.clone(),
        RustSource::default(),
        settings.max_walk,
        usize::MAX,
        ripples,
    );

    // `syn` costs nothing over the wire, so every claimed file is parsed up front rather
    // than only what the walk happens to reach — a file nobody's change touches still gets
    // to say what it defines. Only the rust-analyzer half of reading it stays lazy.
    for path in &ours {
        walk.look(path);
    }
    walk.spread(changed);

    let (source, seen, mentions, contracts, mut notes) = walk.finish();
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

/// A file that won't parse is skipped and spoken about, rather than taking the whole
/// snapshot down with it. Half a review beats none.
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

/// Gives each definition the prose written above it.
///
/// A syntax tree has no comments in it. `///` survives because the language calls it an
/// attribute and hands it over with the item; `//` and `/* */` are thrown away by the lexer
/// before anything here sees them. So a definition arrives owning its declaration and not a
/// word of what was written to explain it, and a change to that explanation is reported as
/// lines belonging to no definition at all — which, in a codebase that explains itself in
/// prose rather than in doc comments, is most of what gets written.
///
/// The rule is the same one every extractor needs, so it's kept in one place and told
/// without knowing what a comment looks like in any language.
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
        /* A file's own module starts where the file does, so there's nothing above it —
         * and reaching for some would take the prose off whatever comes first. */
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

/// Whether this kind of thing can hold others. A property of the language, which is why the
/// extractor is the one to say it: nothing downstream knows that Rust has `impl` blocks.
fn role_of(kind: &str) -> Role {
    match kind {
        "module" | "impl" | "trait" => Role::Container,
        _ => Role::Item,
    }
}

/// What this is written inside: the smallest thing that covers it and isn't it.
///
/// Read straight off the spans, because Rust nests — a method is written inside its `impl`,
/// which is written inside its module. Nothing has to be inferred from names, which is the
/// point: a method's scope names the type it belongs to, not the `impl` block it sits in,
/// so anything working backwards from the scope gets this wrong in the ordinary case.
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
