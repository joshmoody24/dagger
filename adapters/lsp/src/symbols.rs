//! Turning a language server's document symbols into definitions.
//!
//! A server reports where each definition starts and ends, and where its name sits, but
//! not which part of it is the contract and which is the workings. That split has to be
//! guessed, and the guess is made from the symbol's kind: something callable has a
//! signature and then a body, while a type is contract all the way through.

use dagger_lsp_client::Lines;
use serde_json::Value;
use std::ops::Range;

pub struct Symbol {
    pub name: String,
    /// Enclosing symbol names, outermost first. A method carries its class.
    pub scope: Vec<String>,
    pub kind: &'static str,
    pub whole: Range<usize>,
    /// Where the name itself sits, which is where the server has to be asked about it.
    pub name_at: Range<usize>,
    /// The signature, when the kind is one that has a body to be told apart from.
    pub signature: Option<Range<usize>>,
}

impl Symbol {
    /// What callers can see. For anything with a body that's the signature, and for
    /// everything else it's the lot.
    pub fn declaration(&self) -> Range<usize> {
        self.signature.clone().unwrap_or(self.whole.clone())
    }

    pub fn body(&self) -> Option<Range<usize>> {
        let signature = self.signature.clone()?;
        (signature.end < self.whole.end).then_some(signature.end..self.whole.end)
    }
}

/// Flattens the tree a server reports, keeping enclosing names as scope.
pub fn read(symbols: &Value, lines: &Lines) -> Vec<Symbol> {
    let mut found = Vec::new();
    collect(symbols, &[], lines, &mut found);
    found
}

fn collect(symbols: &Value, scope: &[String], lines: &Lines, found: &mut Vec<Symbol>) {
    let Some(symbols) = symbols.as_array() else {
        return;
    };

    for symbol in symbols {
        let Some(name) = symbol["name"].as_str() else {
            continue;
        };
        // Servers that don't do the nested form send a flat list instead, where the range
        // hangs off a location and there's nothing saying where the name itself is.
        let whole =
            span(&symbol["range"], lines).or_else(|| span(&symbol["location"]["range"], lines));
        let Some(whole) = whole else {
            continue;
        };
        let name_at = span(&symbol["selectionRange"], lines).unwrap_or(whole.clone());
        let kind = kind_of(symbol["kind"].as_u64().unwrap_or(0));

        found.push(Symbol {
            name: name.to_string(),
            scope: scope.to_vec(),
            kind,
            signature: splits(kind)
                .then(|| signature(&whole, &name_at, lines))
                .flatten(),
            whole,
            name_at,
        });

        // What lives inside a body is a local, not something anyone reviews on its own.
        // A class's methods are worth descending into; a function's variables aren't.
        if splits(kind) {
            continue;
        }

        let mut inner = scope.to_vec();
        inner.push(name.to_string());
        collect(&symbol["children"], &inner, lines, found);
    }
}

/// Where the signature ends: at the brace that opens the body. Fine for the C-like
/// languages, and for anything else the whole definition stays the contract, which
/// over-reports rather than under-reports.
fn signature(whole: &Range<usize>, name_at: &Range<usize>, lines: &Lines) -> Option<Range<usize>> {
    let text = lines.slice(whole);
    let after_name = name_at.end.saturating_sub(whole.start);
    let brace = text.get(after_name..)?.find('{')? + after_name;
    Some(whole.start..whole.start + brace)
}

/// Only things that are called have workings to hide. A type, a field or a constant is
/// all contract, so a change anywhere in one can break a caller.
fn splits(kind: &str) -> bool {
    matches!(kind, "function" | "method" | "constructor")
}

fn span(range: &Value, lines: &Lines) -> Option<Range<usize>> {
    let start = lines.offset(
        range["start"]["line"].as_u64()? as u32,
        range["start"]["character"].as_u64()? as u32,
    );
    let end = lines.offset(
        range["end"]["line"].as_u64()? as u32,
        range["end"]["character"].as_u64()? as u32,
    );
    (start <= end).then_some(start..end)
}

/// The protocol's numbers, in the words a reader would use. Anything unrecognised keeps
/// its number rather than being dropped, since dagger never reads these anyway.
fn kind_of(kind: u64) -> &'static str {
    match kind {
        1 => "file",
        2 => "module",
        3 => "namespace",
        4 => "package",
        5 => "class",
        6 => "method",
        7 => "property",
        8 => "field",
        9 => "constructor",
        10 => "enum",
        11 => "interface",
        12 => "function",
        13 => "variable",
        14 => "constant",
        15 => "string",
        16 => "number",
        17 => "boolean",
        18 => "array",
        19 => "object",
        20 => "key",
        21 => "null",
        22 => "enum member",
        23 => "struct",
        24 => "event",
        25 => "operator",
        26 => "type parameter",
        _ => "definition",
    }
}
