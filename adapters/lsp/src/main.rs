//! Reads a snapshot through any language server that speaks LSP.
//!
//! Configured with the server to run, and nothing else:
//!
//! ```toml
//! [extractors.settings]
//! server = ["tsc", "--lsp", "--stdio"]
//! ```
//!
//! Works outward from the changed files rather than reading the whole repository: find
//! who refers to a changed definition, read those files, and repeat.

mod symbols;

use anyhow::{Context, Result, bail};
use dagger_core::matching::Extraction;
use dagger_core::model::{Locator, Occurrence, Part, Piece, Role, segments};
use dagger_core::prose::{line_end, line_start, preamble};
use dagger_core::reference::BinderId;
use dagger_lsp_client::walk::{Opened, Reach, Source, Walk, Walked};
use dagger_lsp_client::{self as lsp, Lines, Server};
use dagger_protocol::{Changed, Described, Note, Progress, Request, Response};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;
use std::path::Path;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Settings {
    /// The language server, as you'd type it in a shell.
    server: Vec<String>,
    /// Passed to the server as its initializationOptions.
    #[serde(default)]
    options: Value,
    /// Files this adapter speaks for, when the repo hasn't said.
    #[serde(default)]
    include: Vec<String>,
    /// Backstop on how many files to chase users of. How far a reading goes is
    /// `--ripples`; this only catches a runaway.
    #[serde(default = "files_to_walk")]
    max_walk: usize,
    /// Backstop on how many files to open at all. Opening is cheap and reaches far more
    /// files than walking does, so this sits well above `max_walk`.
    #[serde(default = "files_to_open")]
    max_open: usize,
}

fn files_to_walk() -> usize {
    Reach::WALK
}

fn files_to_open() -> usize {
    Reach::OPEN
}

fn main() -> Result<()> {
    dagger_protocol::serve(answer)
}

fn answer(request: Request) -> Result<Response> {
    match request {
        Request::Describe { settings } => Ok(Response::Described(Described {
            include: settings_of(settings)?.include,
            revisions: None,
            usage: Vec::new(),
        })),
        Request::Extract {
            dir,
            files,
            changed,
            ripples,
            settings,
        } => {
            let (extraction, notes) = extract(
                Path::new(&dir),
                &files,
                &changed,
                ripples,
                settings_of(settings)?,
            )?;
            Ok(Response::Extracted { extraction, notes })
        }
        Request::Materialize { .. } | Request::Resolve { .. } => {
            bail!("this only reads snapshots, it doesn't lay them out")
        }
    }
}

fn settings_of(settings: Value) -> Result<Settings> {
    dagger_protocol::settings(
        settings,
        "dagger-lsp couldn't make sense of its settings; it needs at least a server, \
         like server = [\"tsc\", \"--lsp\", \"--stdio\"]",
    )
}

type File = Opened<symbols::Symbol>;

fn extract(
    dir: &Path,
    files: &[String],
    changed: &[Changed],
    ripples: u32,
    settings: Settings,
) -> Result<(Extraction, Vec<Note>)> {
    let binder = BinderId(
        settings
            .server
            .first()
            .cloned()
            .unwrap_or_else(|| "lsp".to_string()),
    );
    let root = dir.canonicalize()?;
    let ours: BTreeSet<String> = files.iter().cloned().collect();

    eprintln!(
        "{}",
        Progress::StartingServer {
            name: binder.0.clone()
        }
    );
    let server = Server::start(&settings.server, dir, settings.options.clone())?;

    let mut walk = Walk::new(
        server,
        root,
        LspSource::default(),
        Reach {
            binder,
            ours,
            ripples,
            walk_limit: settings.max_walk,
            open_limit: settings.max_open,
        },
    );
    walk.spread(changed);

    let Walked {
        source,
        seen,
        mentions,
        contracts,
        mut notes,
    } = walk.finish();
    notes.extend(source.notes);

    let occurrences = definitions(&seen, &contracts);
    eprintln!(
        "{}",
        Progress::Finished {
            files: seen.len(),
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

/// Finds a file's definitions with `documentSymbol` and reads a contract out of hover.
#[derive(Default)]
struct LspSource {
    /// What was left out of a file and why.
    notes: Vec<Note>,
}

impl Source for LspSource {
    type Item = symbols::Symbol;

    fn open(
        &mut self,
        server: &mut Server,
        root: &Path,
        path: &str,
    ) -> Result<(Lines, Vec<symbols::Symbol>)> {
        let Opened { lines, items, .. } = open(server, root, path, &mut self.notes)?;
        Ok((lines, items))
    }

    fn contract(&self, hover: &Value) -> Option<String> {
        lsp::fenced(hover, None)
    }
}
/// Whether this name is defined elsewhere. Imports are reported as symbols too, and
/// counting one as a definition puts the same thing in the review twice. `None` when the
/// server has no answer, usually an import of something never built; the caller leaves those out.
fn borrowed(
    server: &mut Server,
    at: &Value,
    file: &File,
    symbol: &symbols::Symbol,
) -> Option<bool> {
    /* An import binding is a symbol that is only its name. Asked about an import of
     * something never built, the server points at the import itself, so check first. */
    if symbol.whole == symbol.name_at {
        return Some(true);
    }

    let defined = server.request("textDocument/definition", at.clone()).ok()?;

    let places = match &defined {
        Value::Array(places) if !places.is_empty() => places.clone(),
        Value::Object(_) => vec![defined.clone()],
        _ => return None,
    };

    let borrowed = places.iter().any(|place| {
        let elsewhere = place["uri"]
            .as_str()
            .or_else(|| place["targetUri"].as_str())
            .is_some_and(|uri| !uri.ends_with(&file.path));

        let line = place["range"]["start"]["line"]
            .as_u64()
            .or_else(|| place["targetSelectionRange"]["start"]["line"].as_u64());
        let away = line.is_some_and(|line| {
            let (here, _) = file.lines.position(symbol.name_at.start);
            line as u32 != here
        });

        elsewhere || away
    });
    Some(borrowed)
}

fn open(server: &mut Server, root: &Path, path: &str, notes: &mut Vec<Note>) -> Result<File> {
    let full = root.join(path);
    let text = std::fs::read_to_string(&full).with_context(|| format!("couldn't read {path}"))?;
    let lines = Lines::new(&text);

    server.open(&full, language_of(path), &text)?;
    let reported = server.request(
        "textDocument/documentSymbol",
        json!({ "textDocument": { "uri": lsp::uri(&full) } }),
    )?;

    let file = Opened {
        path: path.to_string(),
        items: symbols::read(&reported, &lines, &path_scope(path)),
        lines,
    };

    let asked: Vec<Option<bool>> = file
        .items
        .iter()
        .map(|symbol| borrowed(server, &position(root, &file, symbol), &file, symbol))
        .collect();
    let unplaced: Vec<&str> = file
        .items
        .iter()
        .zip(&asked)
        .filter(|(_, answer)| answer.is_none())
        .map(|(symbol, _)| symbol.locator.name.as_str())
        .collect();
    if !unplaced.is_empty() {
        notes.push(Note {
            message: format!(
                "{path}: {} could not be traced to a definition, so left out: {}",
                if unplaced.len() == 1 {
                    "a name"
                } else {
                    "names"
                },
                unplaced.join(", ")
            ),
            file: Some(path.to_string()),
        });
    }

    // Two definitions with the same name can't be told apart between snapshots, so only
    // the first keeps it.
    let mut taken = BTreeSet::new();
    let items = file
        .items
        .into_iter()
        .zip(asked)
        .filter(|(_, answer)| *answer == Some(false))
        .map(|(symbol, _)| symbol)
        .filter(|symbol| taken.insert((symbol.scope.clone(), symbol.name.clone())))
        .collect();

    Ok(Opened {
        path: file.path,
        lines: file.lines,
        items,
    })
}

fn position(root: &Path, file: &File, symbol: &symbols::Symbol) -> Value {
    let (line, column) = file.lines.position(symbol.name_at.start);
    json!({
        "textDocument": { "uri": lsp::uri(&root.join(&file.path)) },
        "position": { "line": line, "character": column },
    })
}

/// The file's own scope: its path without the extension, split into segments.
fn path_scope(path: &str) -> Vec<String> {
    segments(&Path::new(path).with_extension(""))
}

fn definitions(
    seen: &BTreeMap<String, File>,
    contracts: &BTreeMap<Locator, String>,
) -> Vec<Occurrence> {
    seen.values()
        .flat_map(|file| {
            let lines = claimed(file);

            let symbols = file.items.iter().map(move |symbol| {
                /* Start from the line start so a leading `export` the server leaves out is
                 * included; it decides whether a change is visible to callers. */
                let text = file.lines.text();
                let body = symbol.body();
                let from = line_start(text, symbol.whole.start);
                let until = match &body {
                    Some(body) => body.start,
                    None => line_end(text, symbol.whole.end),
                };

                let declared = from..until;
                let mut parts =
                    BTreeMap::from([(Part::Type, pieces(file, std::slice::from_ref(&declared)))]);
                if let Some(body) = body {
                    parts.insert(Part::Body, pieces(file, &[body]));
                }
                if let Some(told) = preamble(file.lines.text(), &symbol.whole, &lines) {
                    parts.insert(Part::Docs, pieces(file, &[told]));
                }

                let locator = symbol.locator.clone();
                Occurrence {
                    contract: contracts.get(&locator).cloned(),
                    locator,
                    role: match symbols::holds(symbol.kind) {
                        true => Role::Container,
                        false => Role::Item,
                    },
                    // The file's module holds everything defined at the top level.
                    parent: symbol.parent.clone().or_else(|| module_of(file)),
                    kind: symbol.kind.to_string(),
                    file: file.path.clone(),
                    parts,
                }
            });

            symbols.chain(module(file))
        })
        .collect()
}

/// The file itself, holding whatever none of its definitions do: imports, stray comments,
/// load-time statements. Without it a change that only touches those has nothing to show.
/// It's all body, since nothing refers to a file by name.
fn module(file: &File) -> Option<Occurrence> {
    let leftovers = leftovers(file);
    if leftovers.is_empty() {
        return None;
    }

    Some(Occurrence {
        locator: module_of(file)?,
        role: Role::Container,
        // What holds a file is a project-level question a document symbol request never answers.
        parent: None,
        kind: "module".to_string(),
        file: file.path.clone(),
        parts: BTreeMap::from([(Part::Body, pieces(file, &leftovers))]),
        contract: None,
    })
}

/// What a file's own module is called: its path without the extension.
fn module_of(file: &File) -> Option<Locator> {
    let mut scope = path_scope(&file.path);
    let name = scope.pop()?;
    Some(Locator { scope, name })
}

/// The whole lines each definition sits on. A server's span starts at the name, which
/// would leave `const` and `;` to the module and let the line above read as the
/// definition's documentation.
fn claimed(file: &File) -> Vec<Range<usize>> {
    let text = file.lines.text();
    file.items
        .iter()
        .map(|symbol| line_start(text, symbol.whole.start)..line_end(text, symbol.whole.end))
        .collect()
}

/// The non-blank stretches of a file no definition covers.
fn leftovers(file: &File) -> Vec<Range<usize>> {
    let lines = claimed(file);

    let mut claimed: Vec<Range<usize>> = file
        .items
        .iter()
        .flat_map(|symbol| preamble(file.lines.text(), &symbol.whole, &lines))
        .chain(lines.iter().cloned())
        .collect();
    claimed.sort_by_key(|range| range.start);

    let mut left = Vec::new();
    let mut at = 0usize;
    for range in claimed {
        if range.start > at {
            left.push(at..range.start);
        }
        at = at.max(range.end);
    }
    let end = file.lines.text().len();
    if at < end {
        left.push(at..end);
    }

    left.retain(|range| !file.lines.slice(range).trim().is_empty());
    left
}

fn pieces(file: &File, ranges: &[Range<usize>]) -> Vec<Piece> {
    ranges
        .iter()
        .map(|range| file.lines.piece(&file.path, range))
        .collect()
}

/// Servers need a language id. The extension is a good enough guess; a wrong one only
/// costs that file.
fn language_of(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or("") {
        "ts" => "typescript",
        "tsx" => "typescriptreact",
        "js" => "javascript",
        "jsx" => "javascriptreact",
        "c" | "h" => "c",
        "cc" | "cpp" | "hpp" => "cpp",
        "go" => "go",
        "py" => "python",
        "rs" => "rust",
        _ => "plaintext",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dagger_lsp_client::walk;
    use serde_json::json;

    /// Where a piece of text sits, as a server would say it: a line and a character.
    fn spot(source: &str, at: usize) -> (u32, u32) {
        let before = &source[..at];
        let line = before.matches('\n').count() as u32;
        let column = (at - before.rfind('\n').map_or(0, |found| found + 1)) as u32;
        (line, column)
    }

    /// A symbol as a server reports one, located by searching for its text in the source.
    fn reported(source: &str, name: &str, kind: u64, whole: &str) -> Value {
        let from = source.find(whole).expect("the source should hold it");
        let (line, column) = spot(source, from);
        let (ends, at_end) = spot(source, from + whole.len());
        let (named, at_name) = spot(source, source.find(name).expect("named"));

        json!({
            "name": name,
            "kind": kind,
            "range": {
                "start": { "line": line, "character": column },
                "end": { "line": ends, "character": at_end },
            },
            "selectionRange": {
                "start": { "line": named, "character": at_name },
                "end": { "line": named, "character": at_name + name.len() as u32 },
            },
        })
    }

    fn opened(source: &str, reported: Value) -> File {
        let lines = Lines::new(source);
        Opened {
            path: "src/money.ts".to_string(),
            items: symbols::read(&reported, &lines, &path_scope("src/money.ts")),
            lines,
        }
    }

    fn preambles(file: &File) -> Vec<String> {
        let wholes: Vec<Range<usize>> = file
            .items
            .iter()
            .map(|symbol| symbol.whole.clone())
            .collect();

        file.items
            .iter()
            .filter_map(|symbol| preamble(file.lines.text(), &symbol.whole, &wholes))
            .map(|range| file.lines.slice(&range).to_string())
            .collect()
    }

    #[test]
    fn what_is_written_above_a_definition_belongs_to_it() {
        let source = "/** Money. */\nexport interface Money {\n  pence: number;\n}\n";
        let file = opened(
            source,
            json!([reported(
                source,
                "Money",
                11,
                "export interface Money {\n  pence: number;\n}"
            )]),
        );

        assert_eq!(preambles(&file), vec!["/** Money. */\n"]);
    }

    #[test]
    fn a_blank_line_ends_the_preamble() {
        let source = "// About the file.\n\n/** Money. */\nexport interface Money {}\n";
        let file = opened(
            source,
            json!([reported(source, "Money", 11, "export interface Money {}")]),
        );

        assert_eq!(preambles(&file), vec!["/** Money. */\n"]);
    }

    #[test]
    fn a_preamble_never_takes_another_definitions_line() {
        let source = "export const one = 1;\nexport const two = 2;\n";
        let file = opened(
            source,
            json!([
                reported(source, "one", 13, "export const one = 1;"),
                reported(source, "two", 13, "export const two = 2;"),
            ]),
        );

        assert!(preambles(&file).is_empty());
    }

    #[test]
    fn a_preamble_stops_at_the_line_not_the_name() {
        let source = "/** Both. */\nconst [one, two] = pair();\n";
        let file = opened(
            source,
            json!([
                reported(source, "one", 13, "one"),
                reported(source, "two", 13, "two"),
            ]),
        );

        assert_eq!(preambles(&file), vec!["/** Both. */\n", "/** Both. */\n"]);
    }

    #[test]
    fn a_preamble_inside_something_else_keeps_its_place() {
        let source = "class Money {\n  /** Pence. */\n  pence() {}\n}\n";
        let mut money = reported(source, "Money", 5, source.trim_end());
        money["children"] = json!([reported(source, "pence", 6, "pence() {}")]);
        let file = opened(source, json!([money]));

        assert_eq!(preambles(&file), vec!["  /** Pence. */\n"]);
    }

    #[test]
    fn nothing_is_taken_from_the_definition_above() {
        let source = "/** One. */\nconst one = 1;\nconst two = 2;\n";
        let file = opened(
            source,
            json!([
                reported(source, "one", 13, "one = 1"),
                reported(source, "two", 13, "two = 2"),
            ]),
        );

        assert_eq!(preambles(&file), vec!["/** One. */\n"]);
    }

    #[test]
    fn a_module_keeps_only_what_nobody_else_claims() {
        let source = "import { a } from \"./a\";\n\n/** Money. */\nexport interface Money {}\n";
        let file = opened(
            source,
            json!([reported(source, "Money", 11, "export interface Money {}")]),
        );

        let left: Vec<String> = leftovers(&file)
            .iter()
            .map(|range| file.lines.slice(range).to_string())
            .collect();
        assert_eq!(left, vec!["import { a } from \"./a\";\n\n"]);
    }

    #[test]
    fn a_module_is_named_after_its_file() {
        let source = "import { a } from \"./a\";\n";
        let module = module(&opened(source, json!([]))).expect("a module");

        assert_eq!(module.locator.name, "money");
        assert_eq!(module.locator.scope, vec!["src"]);
    }

    #[test]
    fn a_file_with_nothing_left_over_has_no_module() {
        let source = "export interface Money {}\n";
        let file = opened(
            source,
            json!([reported(source, "Money", 11, "export interface Money {}")]),
        );

        assert!(module(&file).is_none());
    }

    #[test]
    fn a_uri_comes_back_as_a_path_inside_the_snapshot() {
        let root = Path::new("/tmp/dagger-1");

        assert_eq!(
            walk::relative("file:///tmp/dagger-1/src/money.ts", root).as_deref(),
            Some("src/money.ts")
        );
        assert_eq!(walk::relative("file:///elsewhere/money.ts", root), None);
    }

    #[test]
    fn a_language_is_guessed_from_the_extension() {
        assert_eq!(language_of("src/money.ts"), "typescript");
        assert_eq!(language_of("src/App.tsx"), "typescriptreact");
        assert_eq!(language_of("main.rs"), "rust");
        assert_eq!(language_of("Makefile"), "plaintext");
    }
}
