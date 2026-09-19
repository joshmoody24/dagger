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
        if !nameable(name) {
            continue;
        }
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

        if !holds_definitions(kind) {
            continue;
        }

        let mut inner = scope.to_vec();
        inner.push(name.to_string());
        collect(&symbol["children"], &inner, lines, found);
    }
}

/// Whether this is a thing with a name, rather than something a server described in
/// passing.
///
/// Servers report anonymous functions too, under invented labels: `lazyRouter() callback`,
/// `() => {}`, `<anonymous>`. A file wiring up a web server has dozens, all sharing a
/// label, and dagger needs a definition to be addressable by name — two things answering
/// to the same one can't be told apart between snapshots, so they read as a wall of
/// arrivals and departures that nobody wrote.
///
/// Nothing is lost by leaving those out. A reader gets to them through the definition that
/// contains them, which is named.
///
/// Spaces are fine, though. A test is named by the sentence it was given — `applies stacked
/// promos` — and an implementation by what it implements, like `impl Display for Money`.
/// Both are as addressable as any identifier, and both are worth reading.
fn nameable(name: &str) -> bool {
    !name.is_empty() && !name.starts_with('<') && !name.contains(['(', ')'])
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

/// Whether what a server reports inside this are definitions of their own, or parts of it.
///
/// A class holds methods, and a method is a thing somebody reviews. An interface holds
/// fields, and a field is not: it's part of what the interface promises, so changing one
/// changes the interface's contract and breaks whoever relied on it. Reported separately,
/// a field becomes a node with no visible connection to the callers it just broke, and
/// they arrive by the hundred — an interface of a dozen fields is a dozen nodes saying
/// nothing that the interface doesn't say better.
///
/// The extractor for Rust has always worked this way: a struct is one definition covering
/// its whole declaration. This is the same rule, said to a language server.
///
/// It catches a subtler case too. `const faces = (box) => [{ x, y }]` is a variable rather
/// than a function as far as the protocol is concerned, so descending into it turned the
/// keys of the object it returns into definitions.
fn holds_definitions(kind: &str) -> bool {
    matches!(kind, "class" | "namespace" | "module" | "package" | "file")
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const SOURCE: &str = "\
export interface Money {
  amount: number;
}

export function addMoney(a: Money, b: Money): Money {
  return a;
}
";

    /// One symbol as a server reports it: `line`/`character` pairs for the whole thing
    /// and for the name.
    fn reported(name: &str, kind: u64, whole: (u32, u32, u32, u32), at: (u32, u32, u32)) -> Value {
        json!({
            "name": name,
            "kind": kind,
            "range": {
                "start": { "line": whole.0, "character": whole.1 },
                "end": { "line": whole.2, "character": whole.3 },
            },
            "selectionRange": {
                "start": { "line": at.0, "character": at.1 },
                "end": { "line": at.0, "character": at.2 },
            },
        })
    }

    #[test]
    fn a_function_splits_at_the_brace() {
        let lines = Lines::new(SOURCE);
        let symbols = read(
            &json!([reported("addMoney", 12, (4, 0, 6, 1), (4, 16, 24))]),
            &lines,
        );

        let symbol = &symbols[0];
        assert_eq!(
            lines.slice(&symbol.declaration()),
            "export function addMoney(a: Money, b: Money): Money "
        );
        assert_eq!(
            lines.slice(&symbol.body().expect("a function has a body")),
            "{\n  return a;\n}"
        );
    }

    /// A type is contract all the way through, so there's nothing to split off.
    #[test]
    fn an_interface_is_all_declaration() {
        let lines = Lines::new(SOURCE);
        let symbols = read(
            &json!([reported("Money", 11, (0, 0, 2, 1), (0, 17, 22))]),
            &lines,
        );

        assert!(symbols[0].body().is_none());
        assert_eq!(
            lines.slice(&symbols[0].declaration()),
            "export interface Money {\n  amount: number;\n}"
        );
    }

    /* A field is part of what its interface promises, not a definition beside it. Reported
     * separately, a dozen fields become a dozen nodes that say nothing the interface
     * doesn't say better — and none of them show the callers the change just broke. */
    #[test]
    fn what_an_interface_holds_is_part_of_it() {
        let lines = Lines::new(SOURCE);
        let mut interface = reported("Money", 11, (0, 0, 2, 1), (0, 17, 22));
        interface["children"] = json!([reported("amount", 7, (1, 2, 1, 17), (1, 2, 8))]);

        let symbols = read(&json!([interface]), &lines);

        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].name, "Money");
    }

    #[test]
    fn anonymous_things_are_left_out() {
        let lines = Lines::new(SOURCE);
        let reported = json!([
            reported("lazyRouter() callback", 12, (4, 0, 6, 1), (4, 0, 1)),
            reported("() => {}", 12, (4, 0, 6, 1), (4, 0, 1)),
            reported("<anonymous>", 12, (4, 0, 6, 1), (4, 0, 1)),
            reported("addMoney", 12, (4, 0, 6, 1), (4, 16, 24)),
        ]);

        let symbols = read(&reported, &lines);
        let names: Vec<&str> = symbols.iter().map(|symbol| symbol.name.as_str()).collect();
        assert_eq!(names, vec!["addMoney"]);
    }

    /// A test is named by its sentence and an implementation by what it implements. Both
    /// have spaces in them, and both are things a reader came to look at.
    #[test]
    fn a_name_with_spaces_in_it_is_still_a_name() {
        let lines = Lines::new(SOURCE);
        let reported = json!([
            reported("applies stacked promos", 12, (4, 0, 6, 1), (4, 0, 1)),
            reported("impl Display for Money", 5, (0, 0, 2, 1), (0, 0, 1)),
        ]);

        let symbols = read(&reported, &lines);
        let names: Vec<&str> = symbols.iter().map(|symbol| symbol.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["applies stacked promos", "impl Display for Money"]
        );
    }

    /// Locals belong to whoever contains them, not to a reader's list.
    #[test]
    fn what_lives_inside_a_function_is_not_reported() {
        let lines = Lines::new(SOURCE);
        let mut outer = reported("addMoney", 12, (4, 0, 6, 1), (4, 16, 24));
        outer["children"] = json!([reported("total", 13, (5, 2, 5, 12), (5, 8, 13))]);

        assert_eq!(read(&json!([outer]), &lines).len(), 1);
    }

    /// A class's methods are worth listing, unlike a function's variables.
    #[test]
    fn what_lives_inside_a_class_is_reported_with_its_scope() {
        let lines = Lines::new(SOURCE);
        let mut outer = reported("Repo", 5, (0, 0, 2, 1), (0, 17, 21));
        outer["children"] = json!([reported("find", 6, (1, 2, 1, 16), (1, 2, 6))]);

        let symbols = read(&json!([outer]), &lines);
        assert_eq!(symbols.len(), 2);
        assert_eq!(symbols[1].name, "find");
        assert_eq!(symbols[1].scope, vec!["Repo".to_string()]);
    }

    /// Servers that don't do the nested form hang the range off a location instead.
    #[test]
    fn the_flat_form_is_understood_too() {
        let lines = Lines::new(SOURCE);
        let flat = json!([{
            "name": "Money",
            "kind": 11,
            "location": {
                "uri": "file:///money.ts",
                "range": {
                    "start": { "line": 0, "character": 0 },
                    "end": { "line": 2, "character": 1 },
                },
            },
        }]);

        let symbols = read(&flat, &lines);
        assert_eq!(symbols.len(), 1);
        assert_eq!(lines.slice(&symbols[0].whole).lines().count(), 3);
    }
}
