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
use dagger_core::reference::{BinderId, Mention, Site, Target};
use dagger_lsp_client::frontier::{Frontier, Wanted};
use dagger_lsp_client::{self as lsp, Lines, Server};
use dagger_protocol::{Changed, Note, Request, Response};
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

    let mentions = bind(
        dir,
        &parsed,
        changed,
        ripples,
        &mut occurrences,
        &mut notes,
        settings,
    )?;
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

/// Turns what dagger said a changed file differs in into what's worth asking about.
fn wanted_of(changed: &Changed) -> Wanted {
    Wanted::from_spans(&changed.at)
}

/// Asks rust-analyzer who refers to each definition worth asking about, and what each
/// looks like from outside — starting at what changed and following whatever a break can
/// travel through, the same walk the LSP adapter does.
///
/// Parsing every claimed file stays eager: `syn` costs nothing over the wire, and it's
/// what tells a definition what holds it. Only the rust-analyzer conversation — one
/// round trip per question — is worth being lazy about.
fn bind(
    dir: &Path,
    parsed: &[Parsed],
    changed: &[Changed],
    ripples: u32,
    occurrences: &mut [Occurrence],
    notes: &mut Vec<Note>,
    settings: &Settings,
) -> Result<Vec<Mention>> {
    eprintln!("  starting rust-analyzer");
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
    // Indexing is the slow part by a wide margin, so it's worth admitting to.
    eprintln!("  waiting for rust-analyzer to index");
    server.wait_until(|message| {
        message["method"] == "experimental/serverStatus"
            && message["params"]["quiescent"] == serde_json::Value::Bool(true)
    })?;
    let root = dir.canonicalize()?;
    let by_path: BTreeMap<&str, &Parsed> = parsed
        .iter()
        .map(|file| (file.path.as_str(), file))
        .collect();

    let mut mentions = Vec::new();
    let mut contracts: BTreeMap<Locator, String> = BTreeMap::new();

    let mut front = Frontier::default();
    for one in changed
        .iter()
        .filter(|one| by_path.contains_key(one.file.as_str()))
    {
        front.want(&one.file, wanted_of(one), 0);
    }

    while let Some((path, wanted, away)) = front.next() {
        eprintln!("  walked {} of {} files", front.walked(), front.known());
        if front.walked() > settings.max_walk {
            notes.push(Note {
                message: format!(
                    "stopped after chasing users of {} files. This change reaches further \
                     than that, so some of what it affects is missing",
                    settings.max_walk
                ),
                file: None,
            });
            break;
        }

        let Some(file) = by_path.get(path.as_str()) else {
            continue;
        };
        let uri = lsp::uri(&root.join(&file.path));

        let questions: Vec<&items::Found> = file
            .found
            .iter()
            .filter(|found| found.referenceable())
            .filter(|found| wanted.covers(&found.covers, &locator(found)))
            .collect();

        for found in questions {
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

            let (found_mentions, onward) = referring(&referrers, &root, &by_path, found);
            mentions.extend(found_mentions);
            if away + 1 < ripples {
                for (path, from) in onward {
                    front.want(&path, Wanted::named(from), away + 1);
                }
            }
        }
    }

    for occurrence in occurrences.iter_mut() {
        occurrence.contract = contracts.get(&occurrence.locator).cloned();
    }

    eprintln!(
        "  read {} files, found {} definitions",
        parsed.len(),
        occurrences.len()
    );
    Ok(mentions)
}

/// Each place rust-analyzer found, turned into a mention from whichever definition
/// encloses it, alongside which of those places can carry a break onward — the file, and
/// the one definition in it that does. A reference from outside any definition we know
/// about is dropped: there is nothing to hang it on.
fn referring(
    referrers: &serde_json::Value,
    root: &Path,
    by_path: &BTreeMap<&str, &Parsed>,
    to: &items::Found,
) -> (Vec<Mention>, Vec<(String, Locator)>) {
    let Some(places) = referrers.as_array() else {
        return (Vec::new(), Vec::new());
    };

    let mut mentions = Vec::new();
    let mut onward = Vec::new();

    for place in places {
        let found = (|| {
            let path = relative(place["uri"].as_str()?, root)?;
            let file = by_path.get(path.as_str())?;
            let line = place["range"]["start"]["line"].as_u64()? as u32;
            let column = place["range"]["start"]["character"].as_u64()? as u32;
            let at = file.lines.offset(line, column);

            let from = innermost(file, at)?;
            let part = from.part_at(at)?;
            Some((path, from, part, at))
        })();

        let Some((path, from, part, at)) = found else {
            continue;
        };

        mentions.push(Mention {
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
        });

        // Callers of this one can be broken by what broke it, so the trail carries on
        // through this definition, and not through everything else sharing its file. A
        // mention inside a body stops here: nobody outside can tell it changed.
        if part == Part::Type {
            onward.push((path, locator(from)));
        }
    }

    (mentions, onward)
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
