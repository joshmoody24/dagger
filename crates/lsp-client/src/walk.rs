//! A generic walk outward from what changed, for any source that can find a file's
//! definitions and answer what a hover says about one.
//!
//! One driver for every adapter that talks to a language server this way, because the walk
//! itself — spread from a change, ask what a definition looks like and who uses it, follow
//! whatever can carry a break onward — has nothing to do with which language is being read.
//! What differs is how a file's definitions are found: parsing a syntax tree costs nothing,
//! asking a server for `documentSymbol` costs a round trip. That's the one thing a [`Source`]
//! is asked to say, alongside how its server's hover answers are worded.

use crate::frontier::{Frontier, Wanted};
use crate::{Lines, Server};
use anyhow::Result;
use dagger_core::model::{Locator, Part, Span};
use dagger_core::reference::{BinderId, Mention, Site, Target};
use dagger_protocol::{Changed, Note, Progress};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;
use std::path::{Path, PathBuf};

/// One definition, as far as the walk needs to know about it. A source's own type carries
/// everything else the resulting review wants — this is only what's shared between asking a
/// server about a spot and deciding whether a spot is worth asking about at all.
pub trait Item {
    fn locator(&self) -> Locator;
    fn name(&self) -> &str;
    fn whole(&self) -> Range<usize>;
    /// Where the name itself sits, which is where a server has to be asked about it.
    fn name_at(&self) -> Range<usize>;
    /// Whether a server can be asked about this by name. Some things a reader looks at
    /// aren't things code refers to — Rust's modules and `impl` blocks, say — and asking
    /// about one of those lands on whatever happens to be nearby.
    fn referenceable(&self) -> bool {
        true
    }
    fn part_at(&self, at: usize) -> Option<Part>;
}

/// Where a source's definitions come from, and what it can say about a hover.
pub trait Source {
    type Item: Item;

    /// Reads or opens one file, returning what it defines. Cost is the implementation's
    /// business: free and local for a syntax-tree parser, one round trip for a language
    /// server that has to be told about a file before it can be asked anything about it.
    fn open(
        &mut self,
        server: &mut Server,
        root: &Path,
        path: &str,
    ) -> Result<(Lines, Vec<Self::Item>)>;

    /// What a hover response says the definition looks like from outside. Every server
    /// writes this differently.
    fn contract(&self, hover: &Value) -> Option<String>;
}

/// Which files are worth reading, worked outward from what changed, and what's been asked
/// about each so far.
pub struct Walk<S: Source> {
    server: Server,
    root: PathBuf,
    binder: BinderId,
    ours: BTreeSet<String>,
    source: S,
    seen: BTreeMap<String, (Lines, Vec<S::Item>)>,
    mentions: Vec<Mention>,
    contracts: BTreeMap<Locator, String>,
    notes: Vec<Note>,
    /// Files whose users are chased, and files opened at all. Two budgets because they cost
    /// wildly different amounts for a source that has to open a file over the wire — for one
    /// that doesn't, `open_limit` is simply set high enough never to matter.
    walk_limit: usize,
    open_limit: usize,
    /// How far past a changed file to carry on. Walking a file at one remove is what turns
    /// up what sits at two, so the walk stops one short of what's asked for.
    ripples: u32,
}

impl<S: Source> Walk<S> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        server: Server,
        root: PathBuf,
        binder: BinderId,
        ours: BTreeSet<String>,
        source: S,
        walk_limit: usize,
        open_limit: usize,
        ripples: u32,
    ) -> Self {
        Walk {
            server,
            root,
            binder,
            ours,
            source,
            seen: BTreeMap::new(),
            mentions: Vec::new(),
            contracts: BTreeMap::new(),
            notes: Vec::new(),
            walk_limit,
            open_limit,
            ripples,
        }
    }

    /// Opens a file once, keeping what was found. Whether it worked. Cheap to call again: a
    /// file already open is a lookup, not a re-read — which is how a source that opens
    /// everything up front and a walk that opens lazily end up sharing the same cache.
    pub fn look(&mut self, path: &str) -> bool {
        if self.seen.contains_key(path) {
            return true;
        }
        match self.source.open(&mut self.server, &self.root, path) {
            Ok(found) => {
                self.seen.insert(path.to_string(), found);
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

    /// Starts at the files that differ and spreads to whatever a break could reach.
    ///
    /// A file is asked who uses it only if something can travel onward from it: because it
    /// changed, or because it mentions a changed definition somewhere its own callers can
    /// see. Spreading through every reference instead is what made a nine file change
    /// unreadable — one widely used name answers with a thousand places.
    pub fn spread(&mut self, changed: &[Changed]) {
        let mut front = Frontier::default();
        for one in changed
            .iter()
            .filter(|one| self.ours.contains(one.file.as_str()))
        {
            front.want(&one.file, Wanted::from_spans(&one.at), 0);
        }

        /* Which files changed is the same list on both snapshots, so opening them and
         * nobody else — regardless of what a reference walk turns up — is a rule that
         * lands the same way twice. */
        let changed: BTreeSet<String> = changed
            .iter()
            .map(|one| one.file.clone())
            .filter(|file| self.ours.contains(file.as_str()))
            .collect();

        // Open every changed file before asking anything about any of them. A server
        // answers "who uses this" out of the projects it has loaded, and telling it about
        // a file is what loads that file's project.
        for path in changed.iter() {
            self.look(path);
        }

        while let Some((path, wanted, away)) = front.next() {
            eprintln!(
                "{}",
                Progress::Walked {
                    done: front.walked(),
                    known: front.known(),
                    opened: self.seen.len(),
                }
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

            /* Walking a file one step out is what turns up what sits two steps out, so the
             * walk stops one short of how far the reading was asked to go. Everything
             * reached from the last step is still recorded — it just isn't followed. */
            if away + 1 < self.ripples {
                for (path, definition) in self.ask_about(&path, &wanted, &changed, away) {
                    front.want(&path, Wanted::named(definition), away + 1);
                }
            } else {
                self.ask_about(&path, &wanted, &changed, away);
            }
        }
    }

    /// Asks what each definition worth asking about looks like from outside and who uses
    /// it, recording the mentions. Returns the files a break can travel on to.
    fn ask_about(
        &mut self,
        path: &str,
        wanted: &Wanted,
        changed: &BTreeSet<String>,
        away: u32,
    ) -> Vec<(String, Locator)> {
        let questions: Vec<(Value, Locator, String)> = {
            let (lines, found) = &self.seen[path];
            found
                .iter()
                .filter(|item| item.referenceable())
                .filter(|item| wanted.covers(&item.whole(), &item.locator()))
                .map(|item| {
                    (
                        position(&self.root, lines, path, &item.name_at()),
                        item.locator(),
                        item.name().to_string(),
                    )
                })
                .collect()
        };

        let mut onward = Vec::new();
        for (at, to, name) in questions {
            // Hover is a summary written for a person, so it's worth having but not worth
            // trusting on its own — an adapter takes it alongside the written declaration
            // rather than instead of it, which is the source's own business.
            if let Ok(hover) = self.server.request("textDocument/hover", at.clone())
                && let Some(contract) = self.source.contract(&hover)
            {
                self.contracts.insert(to.clone(), contract);
            }

            let mut question = at;
            question["context"] = json!({ "includeDeclaration": false });
            let referrers = match self.server.request("textDocument/references", question) {
                Ok(referrers) => referrers,
                Err(error) => {
                    self.notes.push(Note {
                        message: format!("couldn't find what uses {name}: {error:#}"),
                        file: Some(path.to_string()),
                    });
                    continue;
                }
            };

            onward.extend(self.record(&referrers, &to, changed, away));
        }

        onward
    }

    /// Turns each place a definition is used into a mention, and says which of those places
    /// can carry a break onward — the file, and the one definition in it that does.
    fn record(
        &mut self,
        referrers: &Value,
        to: &Locator,
        changed: &BTreeSet<String>,
        away: u32,
    ) -> Vec<(String, Locator)> {
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
            /* Opening a file is how a mention gets the name of the definition it sits in.
             * Worth doing where the mention can end up in the review — inside a file that
             * changed, or near enough to be reached at the distance asked for — and free
             * where the file is open regardless, which for a source that opens everything
             * up front is every file it claims. */
            let already = self.seen.contains_key(&path);
            let worth_opening = already || changed.contains(&path) || away < self.ripples;
            if !worth_opening || (!already && self.seen.len() >= self.open_limit) {
                continue;
            }
            if !self.look(&path) {
                continue;
            }

            let (lines, found) = &self.seen[&path];
            let at = lines.offset(line, column);
            let Some(from) = innermost(found, at) else {
                continue;
            };
            let Some(part) = from.part_at(at) else {
                continue;
            };

            let inside = from.locator();
            self.mentions.push(Mention {
                from: inside.clone(),
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

            // Callers of this one can be broken by what broke it, so the trail carries on
            // — through this definition, and not through everything else sharing its file.
            // A mention inside a body stops here: nobody outside can tell it changed.
            if part == Part::Type {
                onward.push((path, inside));
            }
        }

        onward
    }

    /// What the walk found, and the source it found it with — an adapter that opened files
    /// of its own to build a full structural picture (rather than only what the walk
    /// touched) gets its source's own state back to read out of.
    #[allow(clippy::type_complexity)]
    pub fn finish(
        self,
    ) -> (
        S,
        BTreeMap<String, (Lines, Vec<S::Item>)>,
        Vec<Mention>,
        BTreeMap<Locator, String>,
        Vec<Note>,
    ) {
        (
            self.source,
            self.seen,
            self.mentions,
            self.contracts,
            self.notes,
        )
    }
}

fn position(root: &Path, lines: &Lines, path: &str, name_at: &Range<usize>) -> Value {
    let (line, column) = lines.position(name_at.start);
    json!({
        "textDocument": { "uri": crate::uri(&root.join(path)) },
        "position": { "line": line, "character": column },
    })
}

fn innermost<I: Item>(found: &[I], at: usize) -> Option<&I> {
    found
        .iter()
        .filter(|item| item.whole().contains(&at))
        .min_by_key(|item| item.whole().len())
}

pub fn relative(uri: &str, root: &Path) -> Option<String> {
    let path = uri.strip_prefix("file://")?;
    Some(
        PathBuf::from(path)
            .strip_prefix(root)
            .ok()?
            .to_string_lossy()
            .into_owned(),
    )
}
