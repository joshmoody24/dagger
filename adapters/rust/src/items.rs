//! Finding the definitions in a parsed file, and where each of their parts sits.

use dagger_core::model::Part;
use proc_macro2::Span;
use quote::ToTokens;
use std::collections::BTreeMap;
use std::ops::Range;
use syn::spanned::Spanned;
use syn::{Attribute, ImplItem, Item, TraitItem};

pub struct Found {
    pub scope: Vec<String>,
    pub name: String,
    /// Where the name itself sits, which is where rust-analyzer has to be asked about it.
    pub name_at: Range<usize>,
    pub kind: &'static str,
    /// Each part as the stretches of source it covers. Several stretches, because a
    /// module's imports sit wherever the language allows them, which in Rust is anywhere.
    pub parts: Parts,
}

pub type Parts = BTreeMap<Part, Vec<Range<usize>>>;

impl Found {
    /// Everything the definition covers, used to spot mentions inside it.
    pub fn extent(&self) -> Range<usize> {
        let stretches = || self.parts.values().flatten();
        let start = stretches().map(|range| range.start).min().unwrap_or(0);
        let end = stretches().map(|range| range.end).max().unwrap_or(0);
        start..end
    }

    pub fn part_at(&self, at: usize) -> Option<Part> {
        // A declaration wins where parts overlap: it's the half a caller can see, and the
        // question being asked is whether a caller could be broken.
        [Part::Type, Part::Body, Part::Docs]
            .into_iter()
            .find(|part| {
                self.parts
                    .get(part)
                    .is_some_and(|ranges| ranges.iter().any(|range| range.contains(&at)))
            })
    }
}

/// Parts from stretches, leaving out the ones a definition doesn't have. A type alias has
/// no body, and most things have no prose.
fn parts(spans: [(Part, Vec<Range<usize>>); 3]) -> Parts {
    spans
        .into_iter()
        .filter(|(_, ranges)| !ranges.is_empty())
        .collect()
}

fn one(range: Range<usize>) -> Vec<Range<usize>> {
    vec![range]
}

pub fn find(items: &[Item], scope: &[String]) -> Vec<Found> {
    items
        .iter()
        .flat_map(|item| from_item(item, scope))
        .collect()
}

/// The module itself, as a definition.
///
/// Without one, a file's imports and its own prose belong to nothing, and a change that
/// only touches those produces a review with nothing in it. With one they land where they
/// belong, and the parts sort out what they mean:
///
/// - `//!` prose is documentation, so it reads as a change without breaking anyone.
/// - `use foo::Bar;` is workings. Callers can't see what a module pulls in for itself.
/// - `pub use foo::Bar;` is contract. Taking one away breaks everyone importing through it,
///   so it belongs where a break can travel from.
pub fn module(
    attrs: &[Attribute],
    items: &[Item],
    path: &[String],
    extent: Range<usize>,
) -> Option<Found> {
    let (name, scope) = path.split_last()?;
    let (contract, workings) = imports(items);

    Some(Found {
        scope: scope.to_vec(),
        name: name.clone(),
        // A file's module has no name written in it. The start is as good a spot as any to
        // ask about, and nothing asks about a module anyway.
        name_at: extent.start..extent.start,
        kind: "module",
        parts: parts([
            (Part::Type, contract),
            (Part::Body, workings),
            (Part::Docs, docs(attrs)),
        ]),
    })
}

/// What a module says about what it brings in and passes on. A `mod` declaration counts
/// too: making one public is publishing whatever is inside it.
fn imports(items: &[Item]) -> (Vec<Range<usize>>, Vec<Range<usize>>) {
    let mut contract = Vec::new();
    let mut workings = Vec::new();

    for item in items {
        let (visibility, span) = match item {
            Item::Use(item) => (&item.vis, range(item.span())),
            Item::ExternCrate(item) => (&item.vis, range(item.span())),
            // Only the declaration, not everything inside it: what's inside has its own
            // definitions, and its own module.
            Item::Mod(item) => (&item.vis, range(item.ident.span())),
            _ => continue,
        };

        match visibility {
            syn::Visibility::Public(_) => contract.push(span),
            _ => workings.push(span),
        }
    }

    (contract, workings)
}

fn from_item(item: &Item, scope: &[String]) -> Vec<Found> {
    match item {
        Item::Fn(function) => vec![callable(
            &function.sig,
            &function.attrs,
            Some(&function.block),
            "fn",
            scope,
        )],
        Item::Struct(item) => vec![whole(item, &item.ident, "struct", &item.attrs, scope)],
        Item::Enum(item) => vec![whole(item, &item.ident, "enum", &item.attrs, scope)],
        Item::Union(item) => vec![whole(item, &item.ident, "union", &item.attrs, scope)],
        Item::Type(item) => vec![whole(item, &item.ident, "type", &item.attrs, scope)],
        Item::Const(item) => vec![whole(item, &item.ident, "const", &item.attrs, scope)],
        Item::Static(item) => vec![whole(item, &item.ident, "static", &item.attrs, scope)],
        Item::Macro(item) => item
            .ident
            .as_ref()
            .map(|ident| whole(item, ident, "macro", &item.attrs, scope))
            .into_iter()
            .collect(),
        Item::Trait(item) => {
            let inner = nest(scope, &item.ident.to_string());
            let mut found = vec![Found {
                scope: scope.to_vec(),
                name: item.ident.to_string(),
                name_at: range(item.ident.span()),
                kind: "trait",
                parts: parts([
                    (Part::Type, one(range(item.ident.span()))),
                    (Part::Body, Vec::new()),
                    (Part::Docs, docs(&item.attrs)),
                ]),
            }];
            found.extend(item.items.iter().filter_map(|member| match member {
                TraitItem::Fn(function) => Some(callable(
                    &function.sig,
                    &function.attrs,
                    function.default.as_ref(),
                    "trait fn",
                    &inner,
                )),
                _ => None,
            }));
            found
        }
        Item::Impl(block) => {
            let inner = nest(scope, &type_name(&block.self_ty));
            block
                .items
                .iter()
                .filter_map(|member| match member {
                    ImplItem::Fn(function) => Some(callable(
                        &function.sig,
                        &function.attrs,
                        Some(&function.block),
                        "method",
                        &inner,
                    )),
                    ImplItem::Const(constant) => Some(Found {
                        scope: inner.clone(),
                        name: constant.ident.to_string(),
                        name_at: range(constant.ident.span()),
                        kind: "assoc const",
                        parts: parts([
                            (Part::Type, one(range(constant.ident.span()))),
                            (Part::Body, one(range(constant.expr.span()))),
                            (Part::Docs, docs(&constant.attrs)),
                        ]),
                    }),
                    _ => None,
                })
                .collect()
        }
        // An inline module is a module like any other, so it gets a definition of its own
        // alongside whatever it holds.
        Item::Mod(item) => match &item.content {
            Some((_, items)) => {
                let path = nest(scope, &item.ident.to_string());
                let mut found = module(&item.attrs, items, &path, range(item.span()))
                    .into_iter()
                    .collect::<Vec<_>>();
                found.extend(find(items, &path));
                found
            }
            None => Vec::new(),
        },
        _ => Vec::new(),
    }
}

/// Something with a signature and, usually, a body behind it.
fn callable(
    signature: &syn::Signature,
    attrs: &[Attribute],
    body: Option<&syn::Block>,
    kind: &'static str,
    scope: &[String],
) -> Found {
    Found {
        scope: scope.to_vec(),
        name: signature.ident.to_string(),
        name_at: range(signature.ident.span()),
        kind,
        parts: parts([
            (Part::Type, one(range(signature.span()))),
            (
                Part::Body,
                body.map(|block| one(range(block.span())))
                    .unwrap_or_default(),
            ),
            (Part::Docs, docs(attrs)),
        ]),
    }
}

/// A definition with nothing hidden, like a struct: all of it is the declaration.
fn whole(
    item: &impl ToTokens,
    ident: &proc_macro2::Ident,
    kind: &'static str,
    attrs: &[Attribute],
    scope: &[String],
) -> Found {
    let docs = docs(attrs);
    let full = range(item.to_token_stream().span());
    let start = docs
        .iter()
        .map(|range| range.end)
        .max()
        .unwrap_or(full.start);

    Found {
        scope: scope.to_vec(),
        name: ident.to_string(),
        name_at: range(ident.span()),
        kind,
        parts: parts([
            (Part::Type, one(start..full.end)),
            (Part::Body, Vec::new()),
            (Part::Docs, docs),
        ]),
    }
}

fn nest(scope: &[String], name: &str) -> Vec<String> {
    let mut nested = scope.to_vec();
    nested.push(name.to_string());
    nested
}

/// Doc comments only. An `#[derive]` belongs to the declaration, not the prose. Runs of
/// them are joined up, since `///` lines are one comment as far as a reader is concerned.
fn docs(attrs: &[Attribute]) -> Vec<Range<usize>> {
    let spans: Vec<Range<usize>> = attrs
        .iter()
        .filter(|attr| attr.path().is_ident("doc"))
        .map(|attr| range(attr.span()))
        .collect();

    let start = spans.iter().map(|span| span.start).min();
    let end = spans.iter().map(|span| span.end).max();
    match (start, end) {
        (Some(start), Some(end)) => one(start..end),
        _ => Vec::new(),
    }
}

fn range(span: Span) -> Range<usize> {
    span.byte_range()
}

fn type_name(ty: &syn::Type) -> String {
    match ty {
        syn::Type::Path(path) => path
            .path
            .segments
            .last()
            .map(|segment| segment.ident.to_string())
            .unwrap_or_else(|| "?".to_string()),
        other => other.to_token_stream().to_string(),
    }
}
