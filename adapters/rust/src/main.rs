//! Reads a Rust snapshot and reports what's defined in it.
//!
//! Binding is done by name alone, so a mention counts only when exactly one
//! definition in the whole snapshot carries that name. Anything ambiguous is left
//! out rather than guessed at, and everything it does report is stamped `naive` so
//! nobody mistakes it for a compiler's word.
//!
//! Only the files dagger hands over are reported on, though they're all parsed
//! together so a mention can find its way to a definition in another file.

mod items;
mod modules;

use anyhow::{Context, Result, bail};
use dagger_core::matching::Extraction;
use dagger_core::model::{Locator, Occurrence, Part, PartText, Span};
use dagger_core::reference::{BinderId, Mention, Site, Target};
use dagger_protocol::{Note, Request, Response};
use proc_macro2::TokenTree;
use quote::ToTokens;
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
        Request::Extract { dir, files } => {
            let (extraction, notes) = extract(Path::new(&dir), &files)?;
            Ok(Response::Extracted { extraction, notes })
        }
        Request::Describe => Ok(Response::Described {
            include: vec!["**/*.rs".to_string()],
        }),
        Request::Materialize { .. } => bail!("this only reads snapshots, it doesn't lay them out"),
    }
}

struct Parsed {
    path: String,
    source: String,
    found: Vec<items::Found>,
}

/// A file that won't parse is skipped and spoken about, rather than taking the whole
/// snapshot down with it. Half a review beats none.
fn extract(dir: &Path, files: &[String]) -> Result<(Extraction, Vec<Note>)> {
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

    let occurrences: Vec<Occurrence> = parsed
        .iter()
        .flat_map(|file| file.found.iter().map(|found| occurrence(file, found)))
        .collect();

    let mentions = bind(&parsed, &occurrences);
    notes.append(&mut modules.notes);
    Ok((
        Extraction {
            occurrences,
            mentions,
        },
        notes,
    ))
}

fn parse(dir: &Path, path: &str, modules: &mut modules::Modules) -> Result<Parsed> {
    let source =
        std::fs::read_to_string(dir.join(path)).with_context(|| format!("couldn't read {path}"))?;
    let file = syn::parse_file(&source).with_context(|| format!("couldn't parse {path}"))?;
    let scope = modules.path_of(dir, path);
    Ok(Parsed {
        path: path.to_string(),
        found: items::find(&file.items, &scope),
        source,
    })
}

fn occurrence(file: &Parsed, found: &items::Found) -> Occurrence {
    let slice = |range: &Range<usize>| PartText {
        text: file.source[range.clone()].to_string(),
        span: Span {
            start: range.start as u32,
            end: range.end as u32,
        },
        file: None,
    };

    let mut parts = BTreeMap::from([(Part::Type, slice(&found.declaration))]);
    if let Some(body) = &found.body {
        parts.insert(Part::Body, slice(body));
    }
    if let Some(docs) = &found.docs {
        parts.insert(Part::Docs, slice(docs));
    }

    Occurrence {
        locator: Locator {
            scope: found.scope.clone(),
            name: found.name.clone(),
        },
        kind: found.kind.to_string(),
        file: file.path.clone(),
        parts,
        contract: None,
    }
}

/// Names that belong to exactly one definition. Anything shared is unbindable by
/// name, so we say nothing rather than picking wrong.
fn unambiguous(occurrences: &[Occurrence]) -> BTreeMap<&str, &Locator> {
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for occurrence in occurrences {
        *counts.entry(occurrence.locator.name.as_str()).or_default() += 1;
    }

    occurrences
        .iter()
        .filter(|occurrence| counts[occurrence.locator.name.as_str()] == 1)
        .map(|occurrence| (occurrence.locator.name.as_str(), &occurrence.locator))
        .collect()
}

fn bind(parsed: &[Parsed], occurrences: &[Occurrence]) -> Vec<Mention> {
    let known = unambiguous(occurrences);
    let mut mentions = Vec::new();

    for file in parsed {
        let Ok(syntax) = syn::parse_file(&file.source) else {
            continue;
        };
        let identifiers = idents(syntax.to_token_stream());

        for found in &file.found {
            let extent = found.extent();
            let from = Locator {
                scope: found.scope.clone(),
                name: found.name.clone(),
            };

            for (name, at) in &identifiers {
                if !extent.contains(&at.start) || *name == found.name {
                    continue;
                }
                let Some(to) = known.get(name.as_str()) else {
                    continue;
                };
                let Some(part) = part_at(found, at.start) else {
                    continue;
                };
                mentions.push(Mention {
                    from: from.clone(),
                    to: Target::Known((*to).clone()),
                    site: Site {
                        part,
                        span: Span {
                            start: at.start as u32,
                            end: at.end as u32,
                        },
                        found_by: BinderId("naive".to_string()),
                    },
                });
            }
        }
    }

    mentions
}

fn part_at(found: &items::Found, at: usize) -> Option<Part> {
    if found.declaration.contains(&at) {
        return Some(Part::Type);
    }
    if found.body.as_ref().is_some_and(|body| body.contains(&at)) {
        return Some(Part::Body);
    }
    if found.docs.as_ref().is_some_and(|docs| docs.contains(&at)) {
        return Some(Part::Docs);
    }
    None
}

/// Every identifier in the file, with where it sits. Reading tokens rather than raw
/// text keeps comments and string literals out of it.
fn idents(tokens: proc_macro2::TokenStream) -> Vec<(String, Range<usize>)> {
    tokens
        .into_iter()
        .flat_map(|tree| match tree {
            TokenTree::Ident(ident) => vec![(ident.to_string(), ident.span().byte_range())],
            TokenTree::Group(group) => idents(group.stream()),
            _ => Vec::new(),
        })
        .collect()
}
