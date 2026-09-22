//! Turns a language server's document symbols into definitions.
//!
//! A server doesn't say which part of a symbol is contract and which is body; that is
//! guessed from the symbol's kind.

use dagger_core::model::{Locator, Part};
use dagger_lsp_client::Lines;
use dagger_lsp_client::walk;
use serde_json::Value;
use std::collections::BTreeMap;
use std::ops::Range;

pub struct Symbol {
    pub name: String,
    /// Enclosing symbol names, outermost first. A method carries its class.
    pub scope: Vec<String>,
    /// The name a break travels by: the file's path folded in ahead of `scope`.
    pub locator: Locator,
    pub kind: &'static str,
    pub whole: Range<usize>,
    /// Where the name itself sits, which is where the server has to be asked about it.
    pub name_at: Range<usize>,
    /// The signature, when the kind is one that has a body to be told apart from.
    pub signature: Option<Range<usize>>,
    /// What this is written inside. `None` at the top of a file; the file's own module
    /// takes those.
    pub parent: Option<Locator>,
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

    /// The declaration wins where parts overlap, since it's the half a caller can see.
    pub fn part_at(&self, at: usize) -> Option<Part> {
        if self.declaration().contains(&at) {
            return Some(Part::Type);
        }
        self.body()
            .is_some_and(|body| body.contains(&at))
            .then_some(Part::Body)
    }
}

impl walk::Item for Symbol {
    fn locator(&self) -> Locator {
        self.locator.clone()
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn whole(&self) -> Range<usize> {
        self.whole.clone()
    }

    fn name_at(&self) -> Range<usize> {
        self.name_at.clone()
    }

    fn part_at(&self, at: usize) -> Option<Part> {
        Symbol::part_at(self, at)
    }
}

/// Flattens the tree a server reports, keeping enclosing names as scope. `file_scope` is
/// the file's own path, folded into every symbol's locator.
pub fn read(symbols: &Value, lines: &Lines, file_scope: &[String]) -> Vec<Symbol> {
    let mut found = Vec::new();
    collect(symbols, file_scope, &[], None, lines, &mut found);
    apart(found)
}

/// Symbols declared in one statement (`const a = 1, b = 2;`) are each reported over the
/// whole statement, so every one of them showed the same text. Each keeps its own stretch:
/// from its name to the next name, the first from the statement's start.
fn apart(found: Vec<Symbol>) -> Vec<Symbol> {
    let mut shared: BTreeMap<(&[String], usize, usize), Vec<usize>> = BTreeMap::new();
    for (at, symbol) in found.iter().enumerate() {
        shared
            .entry((&symbol.scope, symbol.whole.start, symbol.whole.end))
            .or_default()
            .push(at);
    }

    let stretches: Vec<(usize, Range<usize>)> = shared
        .into_values()
        .filter(|together| together.len() > 1)
        .flat_map(|mut together| {
            together.sort_by_key(|&at| found[at].name_at.start);
            let whole = found[together[0]].whole.clone();
            let starts: Vec<usize> = together.iter().map(|&at| found[at].name_at.start).collect();
            together
                .iter()
                .enumerate()
                .map(|(place, &at)| {
                    let from = if place == 0 {
                        whole.start
                    } else {
                        starts[place]
                    };
                    let to = starts.get(place + 1).copied().unwrap_or(whole.end);
                    (at, from..to)
                })
                .collect::<Vec<_>>()
        })
        .collect();

    let mut found = found;
    for (at, stretch) in stretches {
        found[at].whole = stretch;
    }
    found
}

/// Whether this kind of thing can hold others. Same answer as whether to descend into it:
/// if its children are definitions of their own, it's a box around them.
pub fn holds(kind: &str) -> bool {
    holds_definitions(kind)
}

fn collect(
    symbols: &Value,
    file_scope: &[String],
    scope: &[String],
    parent: Option<&Locator>,
    lines: &Lines,
    found: &mut Vec<Symbol>,
) {
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

        let locator = Locator {
            scope: file_scope.iter().chain(scope).cloned().collect(),
            name: name.to_string(),
        };

        found.push(Symbol {
            name: name.to_string(),
            scope: scope.to_vec(),
            locator,
            parent: parent.cloned(),
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
        let holding = Locator {
            scope: scope.to_vec(),
            name: name.to_string(),
        };
        collect(
            &symbol["children"],
            file_scope,
            &inner,
            Some(&holding),
            lines,
            found,
        );
    }
}

/// Whether this is a named thing rather than one a server labelled itself, like
/// `lazyRouter() callback` or `<anonymous>`. Those can't be told apart between snapshots.
/// Spaces are fine: `applies stacked promos` and `impl Display for Money` are real names.
fn nameable(name: &str) -> bool {
    !name.is_empty() && !name.starts_with('<') && !name.contains(['(', ')'])
}

/// The signature ends at the brace that opens the body. For languages without one the
/// whole definition stays contract, which over-reports rather than under-reports.
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

/// Whether a server's children of this kind are definitions of their own. A class's methods
/// are; an interface's fields are part of its contract, and listing them separately hides
/// the callers a change broke. Variables are out so an arrow function's returned keys aren't listed.
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
    fn names_declared_together_each_keep_their_own_stretch() {
        let source = "const A = 1, B = 2;\n";
        let lines = Lines::new(source);
        let symbols = read(
            &json!([
                reported("A", 13, (0, 0, 0, 19), (0, 6, 7)),
                reported("B", 13, (0, 0, 0, 19), (0, 13, 14)),
            ]),
            &lines,
            &[],
        );

        let text = |name: &str| {
            let one = symbols.iter().find(|one| one.name == name).unwrap();
            &source[one.whole.clone()]
        };
        assert_eq!(text("A"), "const A = 1, ");
        assert_eq!(text("B"), "B = 2;");
    }

    #[test]
    fn a_function_splits_at_the_brace() {
        let lines = Lines::new(SOURCE);
        let symbols = read(
            &json!([reported("addMoney", 12, (4, 0, 6, 1), (4, 16, 24))]),
            &lines,
            &[],
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
            &[],
        );

        assert!(symbols[0].body().is_none());
        assert_eq!(
            lines.slice(&symbols[0].declaration()),
            "export interface Money {\n  amount: number;\n}"
        );
    }

    #[test]
    fn what_an_interface_holds_is_part_of_it() {
        let lines = Lines::new(SOURCE);
        let mut interface = reported("Money", 11, (0, 0, 2, 1), (0, 17, 22));
        interface["children"] = json!([reported("amount", 7, (1, 2, 1, 17), (1, 2, 8))]);

        let symbols = read(&json!([interface]), &lines, &[]);

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

        let symbols = read(&reported, &lines, &[]);
        let names: Vec<&str> = symbols.iter().map(|symbol| symbol.name.as_str()).collect();
        assert_eq!(names, vec!["addMoney"]);
    }

    #[test]
    fn a_name_with_spaces_in_it_is_still_a_name() {
        let lines = Lines::new(SOURCE);
        let reported = json!([
            reported("applies stacked promos", 12, (4, 0, 6, 1), (4, 0, 1)),
            reported("impl Display for Money", 5, (0, 0, 2, 1), (0, 0, 1)),
        ]);

        let symbols = read(&reported, &lines, &[]);
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

        assert_eq!(read(&json!([outer]), &lines, &[]).len(), 1);
    }

    /// A class's methods are worth listing, unlike a function's variables.
    #[test]
    fn what_lives_inside_a_class_is_reported_with_its_scope() {
        let lines = Lines::new(SOURCE);
        let mut outer = reported("Repo", 5, (0, 0, 2, 1), (0, 17, 21));
        outer["children"] = json!([reported("find", 6, (1, 2, 1, 16), (1, 2, 6))]);

        let symbols = read(&json!([outer]), &lines, &[]);
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

        let symbols = read(&flat, &lines, &[]);
        assert_eq!(symbols.len(), 1);
        assert_eq!(lines.slice(&symbols[0].whole).lines().count(), 3);
    }
}
