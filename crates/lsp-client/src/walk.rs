//! A generic walk outward from what changed, for any source that can find a file's
//! definitions and read a hover.
//!
//! The walk is the same for every language; only how definitions are found and how hover
//! is worded differ, which is what a [`Source`] says.

use crate::frontier::{Frontier, Wanted};
use crate::{Lines, Server};
use anyhow::Result;
use dagger_core::model::{Locator, Part, Span};
use dagger_core::reference::{ExtractorId, Mention, Site, Target};
use dagger_protocol::{Changed, Note, Progress};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;
use std::path::{Path, PathBuf};

/// One definition, as far as the walk needs to know: enough to ask a server about a spot
/// and to decide whether it's worth asking.
pub trait Item {
    fn locator(&self) -> Locator;
    fn name(&self) -> &str;
    fn whole(&self) -> Range<usize>;
    /// Where the name itself sits, which is where a server has to be asked about it.
    fn name_at(&self) -> Range<usize>;
    /// Whether a server can be asked about this by name. Rust's modules and `impl` blocks
    /// aren't referred to by code, and asking about one lands on whatever is nearby.
    fn referenceable(&self) -> bool {
        true
    }
    fn part_at(&self, at: usize) -> Option<Part>;
}

/// Where a source's definitions come from, and what it can say about a hover.
pub trait Source {
    type Item: Item;

    /// Reads or opens one file, returning what it defines. Free for a syntax-tree parser;
    /// one round trip for a language server.
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

/// How far a walk may go, and who it's walking for.
pub struct Reach {
    /// Whose adapter this is, for the mentions it records.
    pub extractor: ExtractorId,
    /// The files this adapter speaks for. A mention anywhere else is dropped.
    pub ours: BTreeSet<String>,
    /// How far past a changed file to carry on. Walking a file at one remove is what turns
    /// up what sits at two, so the walk stops one short of what's asked for.
    pub ripples: u32,
    /// Backstops on files whose users are chased and files opened at all; how far a review
    /// goes is `ripples`. Two because opening a file is far cheaper than chasing its users.
    pub walk_limit: usize,
    pub open_limit: usize,
}

impl Reach {
    pub const WALK: usize = 100_000;
    pub const OPEN: usize = 500_000;
}

/// A file the walk has looked at, and what it found there.
pub struct Opened<I> {
    pub path: String,
    pub lines: Lines,
    pub items: Vec<I>,
}

/// Which files are worth reading, worked outward from what changed, and what's been asked
/// about each so far.
pub struct Walk<S: Source> {
    server: Server,
    root: PathBuf,
    extractor: ExtractorId,
    ours: BTreeSet<String>,
    source: S,
    seen: BTreeMap<String, Opened<S::Item>>,
    mentions: Vec<Mention>,
    contracts: BTreeMap<Locator, String>,
    notes: Vec<Note>,
    walk_limit: usize,
    open_limit: usize,
    ripples: u32,
}

/// What a walk found, plus the source, since a source may hold state of its own to read out.
pub struct Walked<S: Source> {
    pub source: S,
    pub seen: BTreeMap<String, Opened<S::Item>>,
    pub mentions: Vec<Mention>,
    pub contracts: BTreeMap<Locator, String>,
    pub notes: Vec<Note>,
}

impl<S: Source> Walk<S> {
    pub fn new(server: Server, root: PathBuf, source: S, reach: Reach) -> Self {
        Walk {
            server,
            root,
            extractor: reach.extractor,
            ours: reach.ours,
            source,
            seen: BTreeMap::new(),
            mentions: Vec::new(),
            contracts: BTreeMap::new(),
            notes: Vec::new(),
            walk_limit: reach.walk_limit,
            open_limit: reach.open_limit,
            ripples: reach.ripples,
        }
    }

    /// Opens a file once, keeping what was found; returns whether it worked. A file already
    /// open is a lookup, so calling again is cheap.
    pub fn look(&mut self, path: &str) -> bool {
        if self.seen.contains_key(path) {
            return true;
        }
        match self.source.open(&mut self.server, &self.root, path) {
            Ok((lines, items)) => {
                let opened = Opened {
                    path: path.to_string(),
                    lines,
                    items,
                };
                self.seen.insert(path.to_string(), opened);
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

    /// Starts at the files that differ and spreads to whatever a break could reach. A file
    /// is asked who uses it only if a break can travel onward from it; following every
    /// reference lets one widely used name answer with a thousand places.
    pub fn spread(&mut self, changed: &[Changed]) {
        let mut front = Frontier::default();
        for one in changed
            .iter()
            .filter(|one| self.ours.contains(one.file.as_str()))
        {
            front.want(&one.file, Wanted::from_spans(&one.at), 0);
        }

        /* The changed files are the same list on both snapshots, so opening exactly those
         * gives the same result on each side. */
        let changed: BTreeSet<String> = changed
            .iter()
            .map(|one| one.file.clone())
            .filter(|file| self.ours.contains(file.as_str()))
            .collect();

        // Open every changed file before asking about any: a server answers "who uses this"
        // from the projects it has loaded, and opening a file is what loads its project.
        for path in changed.iter() {
            self.look(path);
        }

        while let Some((path, wanted, away)) = front.take() {
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

            /* Walking a file one step out is what finds what sits two out, so the walk stops
             * one short; the last step is recorded but not followed. */
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
            let Opened { lines, items, .. } = &self.seen[path];
            items
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
            // Hover is written for a person, so the source decides how far to trust it.
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
             * Only worth it where the mention can reach the review, and free if already open. */
            let already = self.seen.contains_key(&path);
            let worth_opening = already || changed.contains(&path) || away < self.ripples;
            if !worth_opening || (!already && self.seen.len() >= self.open_limit) {
                continue;
            }
            if !self.look(&path) {
                continue;
            }

            let Opened { lines, items, .. } = &self.seen[&path];
            let at = lines.offset(line, column);
            let Some(from) = innermost(items, at) else {
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
                    extractor: self.extractor.clone(),
                },
            });

            // Callers of this one can be broken too, so the trail carries on through it.
            // A mention inside a body stops here: nobody outside can see it.
            if part == Part::Contract {
                onward.push((path, inside));
            }
        }

        onward
    }

    pub fn finish(self) -> Walked<S> {
        Walked {
            source: self.source,
            seen: self.seen,
            mentions: self.mentions,
            contracts: self.contracts,
            notes: self.notes,
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The least an item can be: a name, and where it's written.
    struct Spot(&'static str, Range<usize>);

    impl Item for Spot {
        fn locator(&self) -> Locator {
            Locator {
                scope: Vec::new(),
                name: self.0.to_string(),
            }
        }
        fn name(&self) -> &str {
            self.0
        }
        fn whole(&self) -> Range<usize> {
            self.1.clone()
        }
        fn name_at(&self) -> Range<usize> {
            self.1.start..self.1.start
        }
        fn part_at(&self, _: usize) -> Option<Part> {
            Some(Part::Body)
        }
    }

    /* `whole` must cover everything a thing is written across, contents included, or spots
     * between a container's items belong to nothing. */
    #[test]
    fn a_mention_lands_on_the_tightest_thing_around_it() {
        let found = [Spot("module", 0..100), Spot("inner", 40..60)];
        assert_eq!(innermost(&found, 50).map(|item| item.0), Some("inner"));
        assert_eq!(innermost(&found, 20).map(|item| item.0), Some("module"));
        assert_eq!(innermost(&found, 150).map(|item| item.0), None);
    }
}
