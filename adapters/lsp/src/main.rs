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

use crate::symbols::Symbol;
use anyhow::{Context, Result, bail};
use dagger_core::matching::Extraction;
use dagger_core::model::{Locator, Occurrence, Part, Piece, Span};
use dagger_core::reference::{BinderId, Mention, Site, Target};
use dagger_lsp_client::{self as lsp, Lines, Server};
use dagger_protocol::{Note, Request, Response};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::io::{self, Read};
use std::ops::Range;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
struct Settings {
    /// The language server, as you'd type it in a shell.
    server: Vec<String>,
    /// Passed to the server as its initializationOptions.
    #[serde(default)]
    options: Value,
    /// Files this adapter speaks for, when the repo hasn't said.
    #[serde(default)]
    include: Vec<String>,
    /// How many files to read before giving up and saying so. A change to something the
    /// whole repository leans on genuinely does affect everything, and at some point
    /// telling the reader that is more use than carrying on.
    #[serde(default = "a_few_hundred")]
    max_files: usize,
}

fn a_few_hundred() -> usize {
    300
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
        }),
        Request::Extract {
            dir,
            files,
            changed,
            settings,
        } => {
            let (extraction, notes) =
                extract(Path::new(&dir), &files, &changed, settings_of(settings)?)?;
            Ok(Response::Extracted { extraction, notes })
        }
        Request::Materialize { .. } => bail!("this only reads snapshots, it doesn't lay them out"),
    }
}

fn settings_of(settings: Value) -> Result<Settings> {
    serde_json::from_value(settings)
        .context("this adapter needs to be told a server, like server = [\"tsc\", \"--lsp\"]")
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
    changed: &[String],
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

    eprintln!("  starting {}", binder.0);
    let server = Server::start(&settings.server, dir, settings.options.clone())?;

    let mut walk = Walk {
        server,
        root,
        binder,
        ours,
        seen: BTreeMap::new(),
        mentions: Vec::new(),
        contracts: BTreeMap::new(),
        notes: Vec::new(),
        limit: settings.max_files,
    };
    walk.spread(changed);

    let occurrences = definitions(&walk.seen, &walk.contracts);
    eprintln!(
        "  read {} files, found {} definitions",
        walk.seen.len(),
        occurrences.len()
    );

    Ok((
        Extraction {
            occurrences,
            mentions: walk.mentions,
        },
        walk.notes,
    ))
}

struct Walk {
    server: Server,
    root: PathBuf,
    binder: BinderId,
    ours: BTreeSet<String>,
    seen: BTreeMap<String, Opened>,
    mentions: Vec<Mention>,
    contracts: BTreeMap<Locator, String>,
    notes: Vec<Note>,
    limit: usize,
}

impl Walk {
    /// Starts at the files that differ and spreads to whatever a break could reach.
    ///
    /// A file is asked who uses it only if something can travel onward from it: because it
    /// changed, or because it mentions a changed definition somewhere its own callers can
    /// see. A file that merely calls a changed definition from inside a body is opened far
    /// enough to say which definition the call sits in, and no further.
    ///
    /// Spreading through every reference instead is what made a nine file change
    /// unreadable. One widely used name answers with a thousand places; each of those
    /// files holds dozens of definitions; asking all of theirs in turn walks the monorepo.
    fn spread(&mut self, changed: &[String]) {
        let mut queue: VecDeque<String> = changed
            .iter()
            .filter(|path| self.ours.contains(path.as_str()))
            .cloned()
            .collect();
        let mut asked: BTreeSet<String> = BTreeSet::new();

        // Open every changed file before asking anything about any of them. A server
        // answers "who uses this" out of the projects it has loaded, and telling it about
        // a file is what loads that file's project. Asking one package's question while
        // the package that calls it is still unknown gets a truthful answer about a
        // smaller world: the change looks self-contained when it isn't.
        for path in queue.clone() {
            self.look(&path);
        }

        while let Some(path) = queue.pop_front() {
            if !asked.insert(path.clone()) {
                continue;
            }
            if self.seen.len() >= self.limit {
                self.notes.push(Note {
                    message: format!(
                        "stopped after {} files. This change reaches further than that, so \
                         some of what it affects is missing",
                        self.limit
                    ),
                    file: None,
                });
                return;
            }
            if !self.look(&path) {
                continue;
            }

            for onward in self.ask_about(&path) {
                queue.push_back(onward);
            }
        }
    }

    /// Opens a file once, keeping what was found. Whether it worked.
    fn look(&mut self, path: &str) -> bool {
        if self.seen.contains_key(path) {
            return true;
        }
        match open(&mut self.server, &self.root, path) {
            Ok(file) => {
                self.seen.insert(path.to_string(), file);
                true
            }
            Err(error) => {
                self.notes.push(Note {
                    message: format!("skipped it: {error:#}"),
                    file: Some(path.to_string()),
                });
                false
            }
        }
    }

    /// Asks what each definition in this file looks like from outside and who uses it,
    /// recording the mentions. Returns the files a break can travel on to.
    fn ask_about(&mut self, path: &str) -> Vec<String> {
        let questions: Vec<(Value, Locator)> = {
            let file = &self.seen[path];
            file.symbols
                .iter()
                .map(|symbol| (position(&self.root, file, symbol), locator(file, symbol)))
                .collect()
        };

        let mut onward = Vec::new();
        for (at, to) in questions {
            // Hover is a summary written for a person, not a statement of what callers can
            // see, so it's worth having but not worth trusting on its own. Dagger takes it
            // alongside the written declaration rather than instead of it.
            if let Ok(hover) = self.server.request("textDocument/hover", at.clone())
                && let Some(contract) = fenced(&hover)
            {
                self.contracts.insert(to.clone(), contract);
            }

            let mut question = at;
            question["context"] = json!({ "includeDeclaration": false });
            let referrers = match self.server.request("textDocument/references", question) {
                Ok(referrers) => referrers,
                Err(error) => {
                    self.notes.push(Note {
                        message: format!("couldn't find what uses {}: {error:#}", to.name),
                        file: Some(path.to_string()),
                    });
                    continue;
                }
            };

            onward.extend(self.record(&referrers, &to));
        }

        onward
    }

    /// Turns each place a definition is used into a mention, and says which of those
    /// places can carry a break onward.
    fn record(&mut self, referrers: &Value, to: &Locator) -> Vec<String> {
        let places: Vec<(String, u32, u32)> = referrers
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default()
            .iter()
            .filter_map(|place| {
                Some((
                    relative(place["uri"].as_str()?, &self.root)?,
                    place["range"]["start"]["line"].as_u64()? as u32,
                    place["range"]["start"]["character"].as_u64()? as u32,
                ))
            })
            .filter(|(path, _, _)| self.ours.contains(path.as_str()))
            .collect();

        let mut onward = Vec::new();
        for (path, line, column) in places {
            if self.seen.len() >= self.limit || !self.look(&path) {
                continue;
            }

            let file = &self.seen[&path];
            let at = file.lines.offset(line, column);
            let Some(from) = innermost(file, at) else {
                continue;
            };
            let Some(part) = part_at(from, at) else {
                continue;
            };

            self.mentions.push(Mention {
                from: locator(file, from),
                to: Target::Known(to.clone()),
                site: Site {
                    part,
                    span: Span {
                        start: at as u32,
                        end: at as u32,
                    },
                    found_by: self.binder.clone(),
                },
            });

            // Callers of this one can be broken by what broke it, so the trail carries on.
            // A mention inside a body stops here: nobody outside can tell it changed.
            if part == Part::Type {
                onward.push(path);
            }
        }

        onward
    }
}
/// Whether this name belongs to something defined elsewhere. An import is reported as a
/// symbol like any other, but it's a mention of a definition rather than one itself, and
/// counting it would put the same thing in the review twice under two names.
///
/// Asking where the name is defined settles it: a real definition points at itself.
fn borrowed(server: &mut Server, at: &Value, file: &Opened, symbol: &symbols::Symbol) -> bool {
    let Ok(defined) = server.request("textDocument/definition", at.clone()) else {
        return false;
    };

    let places = match &defined {
        Value::Array(places) => places.clone(),
        Value::Null => return false,
        place => vec![place.clone()],
    };

    places.iter().any(|place| {
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
    })
}

fn open(server: &mut Server, root: &Path, path: &str) -> Result<Opened> {
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
        symbols: symbols::read(&reported, &lines),
        lines,
    };

    let ours: Vec<bool> = file
        .symbols
        .iter()
        .map(|symbol| !borrowed(server, &position(root, &file, symbol), &file, symbol))
        .collect();
    let mut keep = ours.iter();
    file.symbols.retain(|_| *keep.next().unwrap_or(&true));

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

fn locator(file: &Opened, symbol: &symbols::Symbol) -> Locator {
    let mut scope: Vec<String> = file
        .path
        .trim_end_matches(|character: char| character != '.')
        .trim_end_matches('.')
        .split('/')
        .map(str::to_string)
        .collect();
    scope.extend(symbol.scope.clone());

    Locator {
        scope,
        name: symbol.name.clone(),
    }
}

fn definitions(
    seen: &BTreeMap<String, Opened>,
    contracts: &BTreeMap<Locator, String>,
) -> Vec<Occurrence> {
    seen.values()
        .flat_map(|file| {
            let wholes: Vec<Range<usize>> = file
                .symbols
                .iter()
                .map(|symbol| symbol.whole.clone())
                .collect();

            let symbols = file.symbols.iter().map(move |symbol| {
                let mut parts =
                    BTreeMap::from([(Part::Type, pieces(file, &[symbol.declaration()]))]);
                if let Some(body) = symbol.body() {
                    parts.insert(Part::Body, pieces(file, &[body]));
                }
                if let Some(told) = preamble(file, symbol, &wholes) {
                    parts.insert(Part::Docs, pieces(file, &[told]));
                }

                let locator = locator(file, symbol);
                Occurrence {
                    contract: contracts.get(&locator).cloned(),
                    locator,
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

    let mut path: Vec<String> = file
        .path
        .rsplit_once('.')
        .map_or(file.path.as_str(), |(stem, _)| stem)
        .split('/')
        .map(str::to_string)
        .collect();
    let name = path.pop()?;

    Some(Occurrence {
        locator: Locator { scope: path, name },
        kind: "module".to_string(),
        file: file.path.clone(),
        parts: BTreeMap::from([(Part::Body, pieces(file, &leftovers))]),
        contract: None,
    })
}

/// What was written just above a definition, which belongs to it.
///
/// A language server reports a definition from its declaration — `export interface Money {`
/// — and says nothing about the comment above explaining what it's for. That comment then
/// belongs to nothing, and falls through to the module, which ends up a pile of prose with
/// gaps where the definitions it describes ought to be. It's documentation: the part of a
/// definition written for whoever uses it, which the model already has a place for.
///
/// Told without knowing a single language's comment syntax: an unbroken run of lines
/// directly above a definition that no other definition claims. Unclaimed is what makes it
/// safe — a line belonging to something else stops the run, so this can never swallow the
/// statement above. A blank line stops it too, which is how anybody writes: prose is
/// against the thing it describes, and separated from whatever came before.
fn preamble(file: &Opened, symbol: &Symbol, claimed: &[Range<usize>]) -> Option<Range<usize>> {
    let text = file.lines.text();
    let mut start = line_start(text, symbol.whole.start);

    loop {
        if start == 0 {
            break;
        }
        let above = line_start(text, start - 1);
        let line = &text[above..start];

        if line.trim().is_empty() {
            break;
        }
        if claimed
            .iter()
            .any(|range| range.start < start && range.end > above)
        {
            break;
        }
        start = above;
    }

    (start < line_start(text, symbol.whole.start)).then_some(start..symbol.whole.start)
}

fn line_start(text: &str, at: usize) -> usize {
    text[..at].rfind('\n').map(|found| found + 1).unwrap_or(0)
}

/// The stretches of a file no definition covers, blank ones left out. A definition can't be
/// asked to account for the space around it.
fn leftovers(file: &Opened) -> Vec<Range<usize>> {
    let wholes: Vec<Range<usize>> = file
        .symbols
        .iter()
        .map(|symbol| symbol.whole.clone())
        .collect();

    let mut claimed: Vec<Range<usize>> = file
        .symbols
        .iter()
        .flat_map(|symbol| preamble(file, symbol, &wholes))
        .chain(wholes.iter().cloned())
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
            file: None,
        })
        .collect()
}

fn innermost(file: &Opened, at: usize) -> Option<&symbols::Symbol> {
    file.symbols
        .iter()
        .filter(|symbol| symbol.whole.contains(&at))
        .min_by_key(|symbol| symbol.whole.len())
}

fn part_at(symbol: &symbols::Symbol, at: usize) -> Option<Part> {
    if symbol.declaration().contains(&at) {
        return Some(Part::Type);
    }
    symbol
        .body()
        .is_some_and(|body| body.contains(&at))
        .then_some(Part::Body)
}

fn relative(uri: &str, root: &Path) -> Option<String> {
    let path = uri.strip_prefix("file://")?;
    Some(
        PathBuf::from(path)
            .strip_prefix(root)
            .ok()?
            .to_string_lossy()
            .into_owned(),
    )
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
            symbols: symbols::read(&reported, &lines),
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
            .filter_map(|symbol| preamble(file, symbol, &wholes))
            .map(|range| file.lines.slice(&range).to_string())
            .collect()
    }

    /* A server reports a definition from its declaration and says nothing about the comment
     * above explaining it. Left there, the comment belongs to nothing and falls through to
     * the module, which ends up a pile of prose with holes where the definitions it
     * describes ought to be. */
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
            relative("file:///tmp/dagger-1/src/money.ts", root).as_deref(),
            Some("src/money.ts")
        );
        assert_eq!(relative("file:///elsewhere/money.ts", root), None);
    }

    #[test]
    fn a_language_is_guessed_from_the_extension() {
        assert_eq!(language_of("src/money.ts"), "typescript");
        assert_eq!(language_of("src/App.tsx"), "typescriptreact");
        assert_eq!(language_of("main.rs"), "rust");
        assert_eq!(language_of("Makefile"), "plaintext");
    }
}
