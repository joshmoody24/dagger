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
    /// How many files to chase users of before giving up and saying so. A change to
    /// something the whole repository leans on genuinely does affect everything, and at
    /// some point telling the reader that is more use than carrying on.
    ///
    /// This is the expensive budget: walking one file means asking the server what each
    /// definition in it looks like from outside and who uses it, which is two requests per
    /// definition. Raise it for a large change and expect to wait.
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
    500
}

fn files_to_open() -> usize {
    5000
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
            settings,
        } => {
            let (extraction, notes) =
                extract(Path::new(&dir), &files, &changed, settings_of(settings)?)?;
            Ok(Response::Extracted { extraction, notes })
        }
        Request::Materialize { .. } | Request::Resolve { .. } => {
            bail!("this only reads snapshots, it doesn't lay them out")
        }
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
        walk_limit: settings.max_walk,
        open_limit: settings.max_open,
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
    /// Files whose users we chase, and files we open at all. Two budgets because they cost
    /// wildly different amounts: with one, the cheap thing spends what the dear thing needs.
    walk_limit: usize,
    open_limit: usize,
}

/* Which files are still to be walked, and which have been.
 *
 * A file earns its place here once, however many times it turns up: one that uses a changed
 * definition in twenty signatures is twenty answers from the server and one file to walk.
 * Letting those through put twenty copies on the queue, which cost nothing to skip later but
 * made the count of what's left meaningless — it went up and down as copies drained.
 */
#[derive(Default)]
struct Frontier {
    queue: VecDeque<String>,
    /// Every path that has ever been queued, walked or not. Membership is what stops a
    /// path being queued twice, so nothing is ever removed from it.
    known: BTreeSet<String>,
    walked: usize,
}

impl Frontier {
    fn from(paths: impl Iterator<Item = String>) -> Self {
        let mut front = Frontier::default();
        for path in paths {
            front.push(path);
        }
        front
    }

    fn push(&mut self, path: String) {
        if self.known.insert(path.clone()) {
            self.queue.push_back(path);
        }
    }

    fn waiting(&self) -> Vec<String> {
        self.queue.iter().cloned().collect()
    }

    #[allow(clippy::should_implement_trait)]
    fn next(&mut self) -> Option<String> {
        let path = self.queue.pop_front()?;
        self.walked += 1;
        Some(path)
    }

    /// How many have been walked, and how many are known to want walking. The second only
    /// ever grows, which is what makes it worth showing: it's what we know, not a guess.
    fn walked(&self) -> usize {
        self.walked
    }

    fn known(&self) -> usize {
        self.known.len()
    }
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
        let mut front = Frontier::from(
            changed
                .iter()
                .filter(|path| self.ours.contains(path.as_str()))
                .cloned(),
        );

        // Open every changed file before asking anything about any of them. A server
        // answers "who uses this" out of the projects it has loaded, and telling it about
        // a file is what loads that file's project. Asking one package's question while
        // the package that calls it is still unknown gets a truthful answer about a
        // smaller world: the change looks self-contained when it isn't.
        for path in front.waiting() {
            self.look(&path);
        }

        while let Some(path) = front.next() {
            /* How far the walk has got. There's no total to count towards — what's left to
             * walk is whatever the files walked so far turn out to mention — so this says
             * how much has been done and how much is known to be left, which is the truth
             * and changes as it goes. Opened files are counted apart because they're the
             * cheap half: a change reaches far more files than it ever walks. */
            eprintln!(
                "  walked {} of {} files, opened {}",
                front.walked(),
                front.known(),
                self.seen.len()
            );
            if front.walked() > self.walk_limit {
                self.notes.push(Note {
                    message: format!(
                        "stopped after chasing users of {} files. This change reaches \
                         further than that, so some of what it affects is missing",
                        self.walk_limit
                    ),
                    file: None,
                });
                return;
            }
            if !self.look(&path) {
                continue;
            }

            for onward in self.ask_about(&path) {
                front.push(onward);
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
            if self.seen.len() >= self.open_limit || !self.look(&path) {
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
                if let Some(told) = preamble(file, symbol, &lines) {
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

    /* How far back this may reach. What's written above a method is inside the class the
     * method is in, so the walk stops below the line the class opens on: prose belongs to
     * whatever it's written inside, and can't be taken from it. */
    let floor = claimed
        .iter()
        .filter(|range| range.start <= symbol.whole.start && range.end >= symbol.whole.end)
        .filter(|range| range.start < line_start(text, symbol.whole.start))
        .map(|range| line_end(text, range.start))
        .max()
        .unwrap_or(0);

    loop {
        if start <= floor {
            break;
        }
        let above = line_start(text, start - 1);
        let line = &text[above..start];

        if line.trim().is_empty() {
            break;
        }
        /* What stops the walk is another definition's line — not the one this sits inside.
         * A method is written inside its class, so the class's lines cover the comment
         * above the method too; letting that stop the walk means a documented method in a
         * class never has any documentation at all. */
        let barred = claimed
            .iter()
            .filter(|range| !(range.start <= symbol.whole.start && range.end >= symbol.whole.end));
        if barred
            .clone()
            .any(|range| range.start < start && range.end > above)
        {
            break;
        }
        start = above;
    }

    /* Stopping at the line, not at the name. A server reports a definition from its own
     * token, which on `const [said, setSaid] = …` is somewhere in the middle of the line —
     * so ending here would hand `const [` to the prose above and leave the declaration to
     * claim the line a second time. */
    let owned = line_start(text, symbol.whole.start);
    (start < owned).then_some(start..owned)
}

fn line_start(text: &str, at: usize) -> usize {
    text[..at].rfind('\n').map(|found| found + 1).unwrap_or(0)
}

fn line_end(text: &str, at: usize) -> usize {
    text[at..]
        .find('\n')
        .map(|found| at + found + 1)
        .unwrap_or(text.len())
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
        .flat_map(|symbol| preamble(file, symbol, &lines))
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

    #[test]
    fn a_file_is_walked_once_however_often_it_turns_up() {
        let mut front = Frontier::from(["a.ts".to_string()].into_iter());
        for _ in 0..20 {
            front.push("b.ts".to_string());
        }

        let mut walked = Vec::new();
        while let Some(path) = front.next() {
            walked.push(path);
        }
        assert_eq!(walked, vec!["a.ts", "b.ts"]);
        assert_eq!(front.walked(), 2);
    }

    #[test]
    fn a_file_already_walked_is_never_queued_again() {
        let mut front = Frontier::from(["a.ts".to_string()].into_iter());
        assert_eq!(front.next().as_deref(), Some("a.ts"));

        front.push("a.ts".to_string());
        assert_eq!(front.next(), None, "a walked file came back around");
    }

    /* What's left to walk is the one number a reader can lean on, so it may not shrink
     * because duplicates drained out of the queue. */
    #[test]
    fn what_is_known_only_ever_grows() {
        let mut front = Frontier::from(["a.ts".to_string(), "b.ts".to_string()].into_iter());
        let mut seen = Vec::new();
        while front.next().is_some() {
            seen.push(front.known());
            front.push("b.ts".to_string());
            front.push("c.ts".to_string());
        }

        assert_eq!(front.known(), 3);
        assert!(
            seen.windows(2).all(|pair| pair[1] >= pair[0]),
            "the count of known files went backwards: {seen:?}"
        );
    }

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
