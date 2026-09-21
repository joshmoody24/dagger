//! What was written just above a definition, which belongs to it.
//!
//! Every extractor has the same hole in it, arrived at from a different direction. A
//! language server reports a definition from its declaration — `export interface Money {`
//! — and says nothing about the comment above explaining what it's for. A parser hands
//! back a syntax tree, and comments were never in it: `///` survives because the language
//! calls it an attribute, and `//` and `/* */` are thrown away by the lexer before anything
//! sees them.
//!
//! Either way that comment belongs to nothing. It falls through to the module, which ends
//! up a pile of prose with gaps where the definitions it describes ought to be — or it
//! falls through to nowhere at all, and a change to it is reported as lines that belong to
//! no definition. It's documentation: the part of a definition written for whoever reads
//! it, which the model already has a place for.
//!
//! Told without knowing a single language's comment syntax, which is what lets every
//! extractor share it: an unbroken run of lines directly above a definition that no other
//! definition claims.

use std::ops::Range;

/// What sits above this definition and belongs to it, if anything does.
///
/// `own` is where the definition starts, and `claimed` is every stretch any definition
/// speaks for — this one included. Unclaimed is what makes the walk safe: a line belonging
/// to something else stops it, so this can never swallow the statement above. A blank line
/// stops it too, which is how anybody writes: prose is set against the thing it describes,
/// and separated from whatever came before.
pub fn preamble(text: &str, own: &Range<usize>, claimed: &[Range<usize>]) -> Option<Range<usize>> {
    let mut start = line_start(text, own.start);

    /* How far back this may reach. What's written above a method is inside the class the
     * method is in, so the walk stops below the line the class opens on: prose belongs to
     * whatever it's written inside, and can't be taken from it. */
    let floor = claimed
        .iter()
        .filter(|range| encloses(range, own))
        .filter(|range| range.start < line_start(text, own.start))
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
            .filter(|range| !encloses(range, own))
            .any(|range| range.start < start && range.end > above);
        if barred {
            break;
        }
        start = above;
    }

    /* Stopping at the line, not at the name. A server reports a definition from its own
     * token, which on `const [said, setSaid] = …` is somewhere in the middle of the line —
     * so ending here would hand `const [` to the prose above and leave the declaration to
     * claim the line a second time. */
    let owned = line_start(text, own.start);
    (start < owned).then_some(start..owned)
}

/// Whether one stretch holds another: what a definition written inside another looks like.
fn encloses(outer: &Range<usize>, inner: &Range<usize>) -> bool {
    outer.start <= inner.start && outer.end >= inner.end
}

/// Where the line holding this spot begins.
pub fn line_start(text: &str, at: usize) -> usize {
    text[..at].rfind('\n').map(|found| found + 1).unwrap_or(0)
}

/// Where the line holding this spot ends, newline included.
pub fn line_end(text: &str, at: usize) -> usize {
    text[at..]
        .find('\n')
        .map(|found| at + found + 1)
        .unwrap_or(text.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Where some text sits, found by looking for it, so a test reads as the code would.
    fn at(source: &str, what: &str) -> Range<usize> {
        let from = source.find(what).expect("the source should hold it");
        from..from + what.len()
    }

    /// The prose above each of these, as it would be read.
    fn above(source: &str, wholes: &[Range<usize>]) -> Vec<String> {
        wholes
            .iter()
            .filter_map(|own| preamble(source, own, wholes))
            .map(|found| source[found].trim().to_string())
            .collect()
    }

    #[test]
    fn a_comment_above_a_definition_belongs_to_it() {
        let source = "// what it is\nfn one() {}\n";
        assert_eq!(
            above(source, &[at(source, "fn one() {}")]),
            ["// what it is"]
        );
    }

    /// The whole point of it being told without knowing any comment syntax.
    #[test]
    fn it_does_not_know_what_a_comment_looks_like() {
        let source = "/* one */\n/* two */\nfn one() {}\n";
        assert_eq!(
            above(source, &[at(source, "fn one() {}")]),
            ["/* one */\n/* two */"]
        );
    }

    #[test]
    fn a_blank_line_ends_it() {
        let source = "// far above\n\n// against it\nfn one() {}\n";
        assert_eq!(
            above(source, &[at(source, "fn one() {}")]),
            ["// against it"]
        );
    }

    /// Unclaimed is what makes the walk safe: prose can never swallow the statement above.
    #[test]
    fn another_definition_ends_it() {
        let source = "fn one() {}\n// against two\nfn two() {}\n";
        let wholes = [at(source, "fn one() {}"), at(source, "fn two() {}")];
        assert_eq!(above(source, &wholes), ["// against two"]);
    }

    /// A method is written inside its class, so the class's lines cover the comment above
    /// the method too. Letting that stop the walk means a documented method never has any
    /// documentation at all.
    #[test]
    fn being_written_inside_something_does_not_end_it() {
        let source = "class One {\n  // what it does\n  two() {}\n}\n";
        let wholes = [
            at(source, "class One {\n  // what it does\n  two() {}\n}"),
            at(source, "two() {}"),
        ];
        assert_eq!(above(source, &wholes), ["// what it does"]);
    }

    /// And it may not reach past the line its own enclosure opens on, which belongs to the
    /// enclosure rather than to anything inside it.
    #[test]
    fn it_stops_below_the_line_it_is_written_inside() {
        let source = "// about the class\nclass One {\n  two() {}\n}\n";
        let whole = at(source, "class One {\n  two() {}\n}");
        let wholes = [whole.clone(), at(source, "two() {}")];
        assert_eq!(above(source, &wholes), ["// about the class"]);
    }

    /// A server reports a definition from its own token, which can be anywhere along the
    /// line. Prose ends where the line starts, or the declaration loses its own opening.
    #[test]
    fn prose_ends_where_the_line_does_not_where_the_name_does() {
        let source = "// about it\nconst [said, setSaid] = make();\n";
        let found = preamble(source, &at(source, "said, setSaid"), &[]).expect("prose");
        assert_eq!(&source[found], "// about it\n");
    }

    #[test]
    fn nothing_above_is_nothing_at_all() {
        let source = "fn one() {}\n";
        assert_eq!(preamble(source, &at(source, "fn one() {}"), &[]), None);
    }
}
