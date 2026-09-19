//! Finding the definitions in a parsed file, and where each of their parts sits.

use proc_macro2::Span;
use quote::ToTokens;
use std::ops::Range;
use syn::spanned::Spanned;
use syn::{Attribute, ImplItem, Item, TraitItem};

pub struct Found {
    pub scope: Vec<String>,
    pub name: String,
    pub kind: &'static str,
    pub docs: Option<Range<usize>>,
    pub declaration: Range<usize>,
    pub body: Option<Range<usize>>,
}

impl Found {
    /// Everything the definition covers, used to spot mentions inside it.
    pub fn extent(&self) -> Range<usize> {
        let start = self
            .docs
            .as_ref()
            .map_or(self.declaration.start, |docs| docs.start);
        let end = self
            .body
            .as_ref()
            .map_or(self.declaration.end, |body| body.end);
        start..end
    }
}

pub fn find(items: &[Item], scope: &[String]) -> Vec<Found> {
    items
        .iter()
        .flat_map(|item| from_item(item, scope))
        .collect()
}

fn from_item(item: &Item, scope: &[String]) -> Vec<Found> {
    match item {
        Item::Fn(function) => vec![Found {
            scope: scope.to_vec(),
            name: function.sig.ident.to_string(),
            kind: "fn",
            docs: docs(&function.attrs),
            declaration: range(function.sig.span()),
            body: Some(range(function.block.span())),
        }],
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
                kind: "trait",
                docs: docs(&item.attrs),
                declaration: range(item.ident.span()),
                body: None,
            }];
            found.extend(item.items.iter().filter_map(|member| match member {
                TraitItem::Fn(function) => Some(Found {
                    scope: inner.clone(),
                    name: function.sig.ident.to_string(),
                    kind: "trait fn",
                    docs: docs(&function.attrs),
                    declaration: range(function.sig.span()),
                    body: function.default.as_ref().map(|block| range(block.span())),
                }),
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
                    ImplItem::Fn(function) => Some(Found {
                        scope: inner.clone(),
                        name: function.sig.ident.to_string(),
                        kind: "method",
                        docs: docs(&function.attrs),
                        declaration: range(function.sig.span()),
                        body: Some(range(function.block.span())),
                    }),
                    ImplItem::Const(constant) => Some(Found {
                        scope: inner.clone(),
                        name: constant.ident.to_string(),
                        kind: "assoc const",
                        docs: docs(&constant.attrs),
                        declaration: range(constant.ident.span()),
                        body: Some(range(constant.expr.span())),
                    }),
                    _ => None,
                })
                .collect()
        }
        Item::Mod(module) => match &module.content {
            Some((_, items)) => find(items, &nest(scope, &module.ident.to_string())),
            None => Vec::new(),
        },
        _ => Vec::new(),
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
    let start = docs.as_ref().map_or(full.start, |docs| docs.end);
    Found {
        scope: scope.to_vec(),
        name: ident.to_string(),
        kind,
        docs,
        declaration: start..full.end,
        body: None,
    }
}

fn nest(scope: &[String], name: &str) -> Vec<String> {
    let mut nested = scope.to_vec();
    nested.push(name.to_string());
    nested
}

/// Doc comments only. An `#[derive]` belongs to the declaration, not the prose.
fn docs(attrs: &[Attribute]) -> Option<Range<usize>> {
    let spans: Vec<Range<usize>> = attrs
        .iter()
        .filter(|attr| attr.path().is_ident("doc"))
        .map(|attr| range(attr.span()))
        .collect();

    let start = spans.iter().map(|span| span.start).min()?;
    let end = spans.iter().map(|span| span.end).max()?;
    Some(start..end)
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
