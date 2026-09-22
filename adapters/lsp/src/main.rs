//! Reads a snapshot through any language server that speaks LSP.
//!
//! Configured with the server to run, and nothing else:
//!
//! ```toml
//! [extractors.settings]
//! server = ["tsc", "--lsp", "--stdio"]
//! ```
//!
//! Works outward from the files that differ rather than reading a whole repository: ask
//! who refers to a changed definition, and whoever does is worth reading too, and worth
//! asking the same question of. That closure is exactly the set a reviewer has to look
//! at, so being lazy and being right turn out to be the same thing.

mod symbols;

use anyhow::{Context, Result, bail};
use dagger_core::matching::Extraction;
use dagger_core::model::{Locator, Occurrence, Part, Piece, Role, Span};
use dagger_core::prose::{line_end, line_start, preamble};
use dagger_core::reference::BinderId;
use dagger_lsp_client::walk::{Reach, Source, Walk, Walked};
use dagger_lsp_client::{self as lsp, Lines, Server};
use dagger_protocol::{Changed, Note, Progress, Request, Response};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, Read};
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
    /// How many files to chase users of before giving up and saying so.
    ///
    /// A backstop, not a setting anybody should have to reach for: how far a reading goes
    /// is `--ripples`, and someone who asks for ten steps and gets six because of a number
    /// they never chose has been told something untrue about their own change. So this
    /// sits high enough to catch a runaway and nothing else.
    #[serde(default = "files_to_walk")]
    max_walk: usize,
    /// How many files to open at all. Opening is cheap — it buys the name of whichever
    /// definition a mention sits inside — and a change of any size reaches far more files
    /// this way than it ever walks, so this sits well above `max_walk`. It's here to stop
    /// a runaway rather than to shape the reading.
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
            include: settings_of(settings)?.include,
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
    serde_json::from_value(settings).context(
        "dagger-lsp couldn't make sense of its settings; it needs at least a server, \
         like server = [\"tsc\", \"--lsp\", \"--stdio\"]",
    )
}

/// A file the adapter has looked at, and what it found there.
struct Opened {
    path: String,
    lines: Lines,
    symbols: Vec<symbols::Symbol>,
}

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
    let seen: BTreeMap<String, Opened> = seen
        .into_iter()
        .map(|(path, (lines, symbols))| {
            (
                path.clone(),
                Opened {
                    path,
                    lines,
                    symbols,
                },
            )
        })
        .collect();

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

/// Discovers a TypeScript-or-whatever file's definitions by asking the language server
/// about it, and reads a contract back out of what it says on hover.
#[derive(Default)]
struct LspSource {
    /// What was left out of a file and why, for whoever reads the review.
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
        let Opened {
            path: _,
            lines,
            symbols,
        } = open(server, root, path, &mut self.notes)?;
        Ok((lines, symbols))
    }

    fn contract(&self, hover: &Value) -> Option<String> {
        fenced(hover)
    }
}
/// Whether this name belongs to something defined elsewhere. An import is reported as a
/// symbol like any other, but it's a mention of a definition rather than one itself, and
/// counting it would put the same thing in the review twice under two names.
///
/// Asking where the name is defined settles it: a real definition points at itself. `None`
/// when the server has no answer at all, which is an import of something never built — a
/// package's compiled output missing from the tree — as often as it's anything else. Read
/// as "defined here", every name in such an import became a definition of its own, one line
/// each; so no answer is no definition, and the caller says so.
fn borrowed(
    server: &mut Server,
    at: &Value,
    file: &Opened,
    symbol: &symbols::Symbol,
) -> Option<bool> {
    /* An import binding is reported as a symbol that is nothing but its name, where a
     * definition has something after its name. Asked where an import of something never
     * built is defined, the server points at the import itself — so this has to be settled
     * before asking. */
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

fn open(server: &mut Server, root: &Path, path: &str, notes: &mut Vec<Note>) -> Result<Opened> {
    let full = root.join(path);
    let text = std::fs::read_to_string(&full).with_context(|| format!("couldn't read {path}"))?;
    let lines = Lines::new(&text);

    server.open(&full, language_of(path), &text)?;
    let reported = server.request(
        "textDocument/documentSymbol",
        json!({ "textDocument": { "uri": lsp::uri(&full) } }),
    )?;

    let mut file = Opened {
        path: path.to_string(),
        symbols: symbols::read(&reported, &lines, &path_scope(path)),
        lines,
    };

    let asked: Vec<Option<bool>> = file
        .symbols
        .iter()
        .map(|symbol| borrowed(server, &position(root, &file, symbol), &file, symbol))
        .collect();
    let unplaced: Vec<&str> = file
        .symbols
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
    let mut keep = asked.iter();
    file.symbols.retain(|_| keep.next() == Some(&Some(false)));

    // A definition has to be addressable by name. Two answering to the same one can't be
    // told apart between snapshots, so the first keeps the name and the rest are dropped
    // rather than left to read as things arriving and departing that nobody wrote.
    let mut taken = BTreeSet::new();
    file.symbols
        .retain(|symbol| taken.insert((symbol.scope.clone(), symbol.name.clone())));

    Ok(file)
}

fn position(root: &Path, file: &Opened, symbol: &symbols::Symbol) -> Value {
    let (line, column) = file.lines.position(symbol.name_at.start);
    json!({
        "textDocument": { "uri": lsp::uri(&root.join(&file.path)) },
        "position": { "line": line, "character": column },
    })
}

/// A file's own scope, its own name a break travels by: its path without the extension,
/// split into segments the way a locator's scope is written everywhere else.
fn path_scope(path: &str) -> Vec<String> {
    path.trim_end_matches(|character: char| character != '.')
        .trim_end_matches('.')
        .split('/')
        .map(str::to_string)
        .collect()
}

fn definitions(
    seen: &BTreeMap<String, Opened>,
    contracts: &BTreeMap<Locator, String>,
) -> Vec<Occurrence> {
    seen.values()
        .flat_map(|file| {
            let lines = claimed(file);

            let symbols = file.symbols.iter().map(move |symbol| {
                /* Shown from the start of its line, because that's where the reader's eye
                 * starts and because what a server leaves out is what matters most: `export`
                 * in front of a definition is the difference between a change nobody can see
                 * and one that breaks every caller. */
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
                    /* Whatever the server said held it, and failing that the file's own
                     * module — which is what holds everything a file defines at the top
                     * level, the same way a class holds its methods. */
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

/// The file itself, holding whatever none of its definitions do.
///
/// A server reports what a file defines, not what else is in it: the imports at the top, a
/// comment sitting between two functions, a statement run at load time. Left out, those
/// belong to nothing, and a change that only touches them produces a review with nothing in
/// it — which for a language whose files start with a dozen imports is most days.
///
/// It goes in as workings rather than contract, because that's what nearly all of it is,
/// and because nothing refers to a file by name for a break to travel through.
fn module(file: &Opened) -> Option<Occurrence> {
    let leftovers = leftovers(file);
    if leftovers.is_empty() {
        return None;
    }

    Some(Occurrence {
        locator: module_of(file)?,
        role: Role::Container,
        // A file is the outermost thing there is here. Whatever holds the file is a
        // question about the project, which a document symbol request never asked.
        parent: None,
        kind: "module".to_string(),
        file: file.path.clone(),
        parts: BTreeMap::from([(Part::Body, pieces(file, &leftovers))]),
        contract: None,
    })
}

/// What a file's own module is called: its path without the extension.
fn module_of(file: &Opened) -> Option<Locator> {
    let mut scope: Vec<String> = file
        .path
        .rsplit_once('.')
        .map_or(file.path.as_str(), |(stem, _)| stem)
        .split('/')
        .map(str::to_string)
        .collect();
    let name = scope.pop()?;
    Some(Locator { scope, name })
}

/// The lines each definition sits on, which is more than the span a server reports.
///
/// A server describes a definition from its name outwards — `EDGE = 24` — and leaves the
/// `const` in front of it and the `;` behind it belonging to nobody. Those crumbs fall to
/// the module, which ends up holding a heap of punctuation with holes where the definitions
/// were. Worse, the line above one definition is then unclaimed, so the line before it
/// reads as its documentation: an import turning up as prose about the thing below it.
///
/// A line is the smallest thing anybody writes on purpose, so a line is what a definition
/// holds.
fn claimed(file: &Opened) -> Vec<Range<usize>> {
    let text = file.lines.text();
    file.symbols
        .iter()
        .map(|symbol| line_start(text, symbol.whole.start)..line_end(text, symbol.whole.end))
        .collect()
}

/// The stretches of a file no definition covers, blank ones left out. A definition can't be
/// asked to account for the space around it.
fn leftovers(file: &Opened) -> Vec<Range<usize>> {
    let lines = claimed(file);

    let mut claimed: Vec<Range<usize>> = file
        .symbols
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

fn pieces(file: &Opened, ranges: &[Range<usize>]) -> Vec<Piece> {
    ranges
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
        .collect()
}

/// Servers want to be told what they're looking at. The extension is as good a guess as
/// any, and a wrong guess only costs us that file.
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

/// The declaration out of a hover's markdown.
///
/// Servers put the signature in a fenced block, sometimes after a block naming the
/// module it lives in, and then a rule followed by documentation. Documentation is where
/// code examples live, and an example is fenced code too, so anything past the rule is
/// left alone: otherwise editing an example in a doc comment reads as breaking every
/// caller.
fn fenced(hover: &Value) -> Option<String> {
    let markdown = hover["contents"]["value"].as_str()?;
    let declaration = markdown.split("\n---").next().unwrap_or(markdown);
    let mut blocks = Vec::new();
    let mut current: Option<Vec<&str>> = None;

    for line in declaration.lines() {
        match (&mut current, line.starts_with("```")) {
            (None, true) => current = Some(Vec::new()),
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

    /// A symbol as a server reports one, found by looking for its own text in the source so
    /// a test can be written as the code a reader would recognise.
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

    fn opened(source: &str, reported: Value) -> Opened {
        let lines = Lines::new(source);
        Opened {
            path: "src/money.ts".to_string(),
            symbols: symbols::read(&reported, &lines, &path_scope("src/money.ts")),
            lines,
        }
    }

    fn preambles(file: &Opened) -> Vec<String> {
        let wholes: Vec<Range<usize>> = file
            .symbols
            .iter()
            .map(|symbol| symbol.whole.clone())
            .collect();

        file.symbols
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

    /* A blank line stops it, which is how anybody writes: prose is against the thing it
     * describes and away from whatever came before. */
    #[test]
    fn a_blank_line_ends_the_preamble() {
        let source = "// About the file.\n\n/** Money. */\nexport interface Money {}\n";
        let file = opened(
            source,
            json!([reported(source, "Money", 11, "export interface Money {}")]),
        );

        assert_eq!(preambles(&file), vec!["/** Money. */\n"]);
    }

    /* The part that makes it safe in a language nobody wrote a rule for: a line that
     * belongs to another definition stops the run, so this can never swallow the statement
     * above it. */
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

    /* What the module is left holding: the file's own prose and its imports, and not the
     * comments that belong to the definitions below them. */
    /* A server reports a definition from its own name, which on a destructured binding
     * sits in the middle of its line. Ending the prose there handed `const [` to the
     * comment above and left the declaration to claim the line a second time. */
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

    /* Prose sits where the thing it describes sits, so what's taken keeps its indentation
     * and doesn't reach back to the margin. */
    #[test]
    fn a_preamble_inside_something_else_keeps_its_place() {
        let source = "class Money {\n  /** Pence. */\n  pence() {}\n}\n";
        let mut money = reported(source, "Money", 5, source.trim_end());
        money["children"] = json!([reported(source, "pence", 6, "pence() {}")]);
        let file = opened(source, json!([money]));

        assert_eq!(preambles(&file), vec!["  /** Pence. */\n"]);
    }

    /* Two definitions written one after the other with nothing between them: the second
     * takes nothing, because the line above it belongs to the first. */
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

    /* Nothing to say, nothing to report: a file where every line belongs to a definition
     * has no module of its own to read. */
    #[test]
    fn a_file_with_nothing_left_over_has_no_module() {
        let source = "export interface Money {}\n";
        let file = opened(
            source,
            json!([reported(source, "Money", 11, "export interface Money {}")]),
        );

        assert!(module(&file).is_none());
    }

    /* Hover is markdown: the signature in a fenced block, then a rule, then documentation.
     * Documentation is where examples live, and an example is fenced code too — so reading
     * past the rule means editing an example reads as breaking every caller. */
    #[test]
    fn a_contract_stops_at_the_documentation() {
        let hover = json!({
            "contents": { "value": "```ts\nfunction add(a: number): number\n```\n---\nAdds.\n\n```ts\nadd(1)\n```" }
        });

        assert_eq!(
            fenced(&hover).as_deref(),
            Some("function add(a: number): number")
        );
    }

    /* Servers often put the module the definition lives in in a block of its own first. */
    #[test]
    fn the_last_block_before_the_rule_is_the_declaration() {
        let hover = json!({
            "contents": { "value": "```ts\nmodule \"money\"\n```\n```ts\nconst pence: number\n```" }
        });

        assert_eq!(fenced(&hover).as_deref(), Some("const pence: number"));
    }

    #[test]
    fn a_hover_with_nothing_fenced_says_nothing() {
        assert_eq!(fenced(&json!({ "contents": { "value": "Adds." } })), None);
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
