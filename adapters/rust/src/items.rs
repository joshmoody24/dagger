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

    /// Whether a language server can be asked about this by name.
    ///
    /// A module and an implementation block are things a reader looks at, but not things
    /// code refers to: nothing writes the name of a file, and nothing calls an `impl`.
    /// Neither has a name written down to point at either, so asking about one lands on
    /// whatever happens to be nearby — for an `impl` that's the type it's about, which
    /// comes back with the type's documentation and the type's callers, both filed under
    /// the wrong definition.
    pub fn referenceable(&self) -> bool {
        !matches!(self.kind, "module" | "impl")
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

/// What a caller can see, which is the whole of an item but its prose and its workings.
///
/// Taken from where the item starts rather than from where its name does, because what sits
/// between the two is `pub` — and losing that means making something public reads as a
/// change to its documentation, which is the mildest mark there is standing in for the most
/// breaking edit there is. An attribute written above the prose is kept for the same reason:
/// it's part of the declaration, and left out it would belong to nothing at all.
fn declared(outer: &Range<usize>, prose: &[Range<usize>], until: usize) -> Vec<Range<usize>> {
    let mut ranges = Vec::new();
    if let Some(first) = prose.first().filter(|first| first.start > outer.start) {
        ranges.push(outer.start..first.start);
    }

    let after = prose
        .iter()
        .map(|range| range.end)
        .max()
        .unwrap_or(outer.start);
    if after < until {
        ranges.push(after..until);
    }
    ranges
}

pub fn find(items: &[Item], scope: &[String]) -> Vec<Found> {
    merged(
        items
            .iter()
            .flat_map(|item| from_item(item, scope))
            .collect(),
    )
}

/// Anything answering to the same name becomes one definition holding both stretches.
///
/// A type's methods can be split across as many `impl` blocks as somebody felt like, and
/// each one is a stretch of the same thing rather than a thing of its own. Leaving them as
/// two definitions with one name would mean neither could be told from the other between
/// snapshots, which is the rule extractors are asked to keep: a definition has to be
/// addressable by name.
fn merged(found: Vec<Found>) -> Vec<Found> {
    let mut out: Vec<Found> = Vec::with_capacity(found.len());

    for one in found {
        match out
            .iter_mut()
            .find(|kept| kept.scope == one.scope && kept.name == one.name)
        {
            Some(kept) => {
                for (part, mut ranges) in one.parts {
                    kept.parts.entry(part).or_default().append(&mut ranges);
                }
                for ranges in kept.parts.values_mut() {
                    ranges.sort_by_key(|range| range.start);
                }
            }
            None => out.push(one),
        }
    }

    out
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

    // Prose reaches whatever it introduces, so the blank line under it isn't a hole.
    let told = docs(attrs);
    let first = contract
        .iter()
        .chain(workings.iter())
        .map(|span| span.start)
        .min();
    let told = match (told.first(), first) {
        (Some(prose), Some(first)) if first > prose.end => one(prose.start..first),
        _ => told,
    };

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
            (Part::Docs, told),
        ]),
    })
}

/// What a module says about what it brings in and passes on. A `mod` declaration counts
/// too: making one public is publishing whatever is inside it.
fn imports(items: &[Item]) -> (Vec<Range<usize>>, Vec<Range<usize>>) {
    let mut found: Vec<(usize, bool, Range<usize>)> = Vec::new();

    for (at, item) in items.iter().enumerate() {
        let (visibility, span) = match item {
            Item::Use(item) => (&item.vis, range(item.span())),
            Item::ExternCrate(item) => (&item.vis, range(item.span())),
            // The header, not everything inside it: what's inside has its own definitions,
            // and its own module.
            Item::Mod(item) => {
                let whole = range(item.span());
                let header = match &item.content {
                    Some(_) => whole.start..range(item.ident.span()).end,
                    None => whole,
                };
                (&item.vis, header)
            }
            _ => continue,
        };

        found.push((at, matches!(visibility, syn::Visibility::Public(_)), span));
    }

    // A run of declarations is one stretch of the file, so each reaches the next rather than
    // stopping at its own semicolon. Left tight, the line ending between two of them belongs
    // to nothing, and a reader is told something was left out between every pair of imports.
    // Only a run: anything else in between is somebody else's, and the gap there is real.
    for at in 0..found.len().saturating_sub(1) {
        let (here, next) = (found[at].0, found[at + 1].0);
        let (ends, starts) = (found[at].2.end, found[at + 1].2.start);
        if next == here + 1 && starts > ends {
            found[at].2.end = starts;
        }
    }

    let (public, private): (Vec<_>, Vec<_>) = found.into_iter().partition(|(_, public, _)| *public);
    let spans = |of: Vec<(usize, bool, Range<usize>)>| of.into_iter().map(|(_, _, span)| span);
    (spans(public).collect(), spans(private).collect())
}

fn from_item(item: &Item, scope: &[String]) -> Vec<Found> {
    match item {
        Item::Fn(function) => vec![callable(
            range(function.span()),
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
                    range(function.span()),
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
            let inner = nest(scope, &implementing(block));
            let mut found = vec![implementation(block, scope)];

            found.extend(block.items.iter().filter_map(|member| match member {
                ImplItem::Fn(function) => Some(callable(
                    range(function.span()),
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
            }));

            found
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

/// An implementation block, as a definition.
///
/// `impl Display for Money` is a claim about Money that callers rely on: it makes Money
/// usable wherever something displayable is wanted, and taking it away breaks them. It can
/// also sit in a different file from Money, or a different crate, so it can't just be
/// folded into what Money says about itself.
///
/// The braces are claimed along with the header so that nothing in the file belongs to
/// nobody. What's between them has definitions of its own.
fn implementation(block: &syn::ItemImpl, scope: &[String]) -> Found {
    let header = range(block.impl_token.span()).start;
    let signed = block
        .generics
        .where_clause
        .as_ref()
        .map(|clause| range(clause.span()))
        .unwrap_or_else(|| range(block.self_ty.span()));
    let full = range(block.span());
    let told = docs(&block.attrs);

    Found {
        scope: scope.to_vec(),
        name: format!("impl {}", implementing(block)),
        name_at: range(block.self_ty.span()),
        kind: "impl",
        parts: parts([
            (
                Part::Type,
                vec![header..signed.end, full.end.saturating_sub(1)..full.end],
            ),
            (Part::Body, Vec::new()),
            (
                Part::Docs,
                told.first()
                    .map(|first| one(first.start..header))
                    .unwrap_or_default(),
            ),
        ]),
    }
}

/// What the block implements, written the way Rust writes it when it has to say which of
/// two implementations it means: `Money as Display`.
///
/// Spelling out the trait is what keeps `Display::fmt` and `Debug::fmt` apart. Scoping both
/// under plain `Money` gives them one name between them, and two definitions answering to
/// one name can't be told apart between snapshots.
fn implementing(block: &syn::ItemImpl) -> String {
    let subject = type_name(&block.self_ty);
    match &block.trait_ {
        Some((_, path, _)) => match path.segments.last() {
            Some(trait_) => format!("{subject} as {}", trait_.ident),
            None => subject,
        },
        None => subject,
    }
}

/// Something with a signature and, usually, a body behind it.
///
/// The parts are butted up against each other rather than each being as tight as it could
/// be. A tight range leaves the newline between a doc comment and the signature belonging to
/// nothing, and a reader shown the definition afterwards is told there's something missing
/// in the gap. There isn't: it's a line ending. Parts divide a definition up; they aren't
/// supposed to leave crumbs between them.
fn callable(
    outer: Range<usize>,
    signature: &syn::Signature,
    attrs: &[Attribute],
    body: Option<&syn::Block>,
    kind: &'static str,
    scope: &[String],
) -> Found {
    let prose = docs(attrs);
    let workings = body.map(|block| range(block.span()));
    let until = workings
        .as_ref()
        .map(|body| body.start)
        .unwrap_or(outer.end);

    Found {
        scope: scope.to_vec(),
        name: signature.ident.to_string(),
        name_at: range(signature.ident.span()),
        kind,
        parts: parts([
            (Part::Type, declared(&outer, &prose, until)),
            (Part::Body, workings.map(one).unwrap_or_default()),
            (Part::Docs, prose),
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
    let prose = docs(attrs);
    let full = range(item.to_token_stream().span());

    Found {
        scope: scope.to_vec(),
        name: ident.to_string(),
        name_at: range(ident.span()),
        kind,
        parts: parts([
            (Part::Type, declared(&full, &prose, full.end)),
            (Part::Body, Vec::new()),
            (Part::Docs, prose),
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Everything one file yields, the way the adapter asks for it: the module first, then
    /// whatever it holds.
    fn read(source: &str) -> Vec<Found> {
        let file = syn::parse_file(source).expect("the source should parse");
        let scope = vec!["thing".to_string()];
        let mut found: Vec<Found> = module(&file.attrs, &file.items, &scope, 0..source.len())
            .into_iter()
            .collect();
        found.extend(find(&file.items, &scope));
        found
    }

    fn named<'a>(found: &'a [Found], name: &str) -> &'a Found {
        found
            .iter()
            .find(|one| one.name == name)
            .unwrap_or_else(|| panic!("nothing called {name} in {:?}", names(found)))
    }

    fn names(found: &[Found]) -> Vec<&str> {
        found.iter().map(|one| one.name.as_str()).collect()
    }

    /// What a reader would be shown, with a marker wherever the parts don't meet. Anything
    /// between two pieces of the same definition is a hole the page has to explain.
    fn shown(source: &str, found: &Found) -> String {
        let mut pieces: Vec<&Range<usize>> = found.parts.values().flatten().collect();
        pieces.sort_by_key(|range| range.start);

        let mut out = String::new();
        let mut last = None;
        for piece in pieces {
            if last.is_some_and(|end| piece.start > end) {
                out.push('…');
            }
            out.push_str(&source[piece.clone()]);
            last = Some(piece.end);
        }
        out
    }

    /* The bug this file kept having: a part stopping exactly where the text of the thing
     * stops, leaving the newline between it and the next part belonging to nobody. A reader
     * is then told something was left out, and nothing was. */
    #[test]
    fn the_parts_of_a_definition_meet() {
        let source = "/// Adds them up.\npub fn add(a: u8, b: u8) -> u8 {\n    a + b\n}\n";
        let found = read(source);

        assert_eq!(shown(source, named(&found, "add")), source.trim_end());
    }

    #[test]
    fn prose_is_docs_and_the_signature_is_the_contract() {
        let source = "/// Adds them up.\npub fn add(a: u8) -> u8 {\n    a\n}\n";
        let found = read(source);
        let add = named(&found, "add");

        assert!(source[add.parts[&Part::Docs][0].clone()].contains("Adds them up"));
        assert!(source[add.parts[&Part::Type][0].clone()].contains("pub fn add(a: u8) -> u8"));
        assert!(source[add.parts[&Part::Body][0].clone()].starts_with('{'));
    }

    /* A struct is all contract: change any of it and a caller can break, so there's no
     * body to tell apart. */
    #[test]
    fn a_struct_has_no_workings() {
        let source = "/// Money.\npub struct Money {\n    pub pence: u8,\n}\n";
        let found = read(source);
        let money = named(&found, "Money");

        assert!(!money.parts.contains_key(&Part::Body));
        assert!(source[money.parts[&Part::Type][0].clone()].contains("pub pence"));
        assert_eq!(shown(source, money), source.trim_end());
    }

    /* A field is part of the struct, not a definition beside it. */
    #[test]
    fn a_struct_holds_no_definitions_of_its_own() {
        let found = read("pub struct Money {\n    pub pence: u8,\n}\n");
        assert_eq!(names(&found), vec!["thing", "Money"]);
    }

    /* Declarations one after another are one stretch of the file. Claiming only the name of
     * each left the `;` and the newline between them belonging to nobody, so a module of
     * twenty of them read as twenty fragments. */
    #[test]
    fn a_run_of_declarations_is_one_stretch() {
        let source = "pub mod one;\npub mod two;\npub mod three;\n";
        let found = read(source);

        assert_eq!(shown(source, named(&found, "thing")), source.trim_end());
    }

    /* Prose at the top of a file introduces whatever comes next, so it reaches it. */
    #[test]
    fn a_modules_prose_reaches_what_it_introduces() {
        let source = "//! About this.\n\nuse std::fmt;\n";
        let found = read(source);

        assert_eq!(shown(source, named(&found, "thing")), source.trim_end());
    }

    /* What a module passes on is contract; what it keeps for itself is workings. */
    #[test]
    fn a_public_import_is_contract_and_a_private_one_is_not() {
        let source = "pub use one::Thing;\nuse two::Other;\n";
        let found = read(source);
        let module = named(&found, "thing");

        assert!(source[module.parts[&Part::Type][0].clone()].contains("pub use one::Thing"));
        assert!(source[module.parts[&Part::Body][0].clone()].contains("use two::Other"));
    }

    /* An implementation is a claim about a type that callers rely on, so it's a definition.
     * What's between its braces has definitions of its own, so it claims the header and the
     * closing brace and leaves the middle alone — the one place a gap is honest. */
    #[test]
    fn an_implementation_claims_its_header_and_its_brace() {
        let source = "impl Money {\n    pub fn pence(&self) -> u8 {\n        0\n    }\n}\n";
        let found = read(source);

        assert_eq!(names(&found), vec!["thing", "impl Money", "pence"]);
        assert_eq!(shown(source, named(&found, "impl Money")), "impl Money…}");
    }

    /* Spelling out the trait is what keeps `Display::fmt` and `Debug::fmt` apart: scoped
     * under plain `Money` they'd share one name, and two definitions answering to one name
     * can't be told apart between snapshots. */
    #[test]
    fn an_implementation_says_what_it_implements() {
        let found = read("impl fmt::Display for Money {\n    fn fmt(&self) {}\n}\n");

        assert_eq!(named(&found, "impl Money as Display").kind, "impl");
        assert_eq!(
            named(&found, "fmt").scope,
            vec!["thing", "Money as Display"]
        );
    }

    /* A type's methods can be split across as many blocks as somebody felt like. Left as
     * two definitions with one name, neither could be told from the other. */
    #[test]
    fn two_blocks_for_one_type_are_one_definition() {
        let found =
            read("impl Money {\n    fn a(&self) {}\n}\nimpl Money {\n    fn b(&self) {}\n}\n");

        assert_eq!(
            found.iter().filter(|one| one.name == "impl Money").count(),
            1
        );
        assert_eq!(names(&found), vec!["thing", "impl Money", "a", "b"]);
    }

    /* Nothing writes the name of a file and nothing calls an `impl`, so neither has a name
     * written down to ask a language server about. Asking anyway lands on whatever is
     * nearby — for an `impl`, the type it's about, whose documentation and callers then
     * arrive filed under the wrong definition. */
    #[test]
    fn a_module_and_an_implementation_cant_be_asked_about() {
        let found = read("impl Money {\n    fn a(&self) {}\n}\n");

        assert!(!named(&found, "thing").referenceable());
        assert!(!named(&found, "impl Money").referenceable());
        assert!(named(&found, "a").referenceable());
    }

    /* An inline module is a module like any other, and what's inside it is scoped under it
     * rather than beside it. */
    #[test]
    fn an_inline_module_holds_its_own() {
        let found = read("mod inner {\n    pub fn deep() {}\n}\n");

        assert_eq!(named(&found, "inner").kind, "module");
        assert_eq!(named(&found, "deep").scope, vec!["thing", "inner"]);
    }

    /* Where a caller can see it and where it can't. */
    #[test]
    fn a_declaration_wins_where_parts_overlap() {
        let source = "pub fn add(a: u8) -> u8 {\n    a\n}\n";
        let found = read(source);
        let add = named(&found, "add");

        assert_eq!(add.part_at(0), Some(Part::Type));
        assert_eq!(add.part_at(source.find("    a").unwrap()), Some(Part::Body));
    }
}
