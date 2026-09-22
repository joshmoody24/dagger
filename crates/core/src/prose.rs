//! Attaches the comment written directly above a definition to that definition.
//!
//! Extractors don't report comments (language servers start at the declaration, parsers
//! drop them), so it's done once here, without knowing any language's comment syntax.

use std::ops::Range;

/// The unbroken run of lines directly above `own` that no other definition claims.
///
/// `claimed` is every range some definition speaks for, `own` included.
pub fn preamble(text: &str, own: &Range<usize>, claimed: &[Range<usize>]) -> Option<Range<usize>> {
    let mut start = line_start(text, own.start);

    // A comment above the enclosing definition's opening line belongs to the enclosure,
    // not to anything inside it.
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
        // The enclosing definition's range covers the comment too, so it must not stop the walk.
        let barred = claimed
            .iter()
            .filter(|range| !encloses(range, own))
            .any(|range| range.start < start && range.end > above);
        if barred {
            break;
        }
        start = above;
    }

    // End at the line start, not at `own.start`: a server may report `const [said, setSaid]`
    // from a token in the middle of the line.
    let owned = line_start(text, own.start);
    (start < owned).then_some(start..owned)
}

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

    /// Where `what` sits in `source`.
    fn at(source: &str, what: &str) -> Range<usize> {
        let from = source.find(what).expect("the source should hold it");
        from..from + what.len()
    }

    /// The prose above each of these.
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

    #[test]
    fn another_definition_ends_it() {
        let source = "fn one() {}\n// against two\nfn two() {}\n";
        let wholes = [at(source, "fn one() {}"), at(source, "fn two() {}")];
        assert_eq!(above(source, &wholes), ["// against two"]);
    }

    /// The enclosing class's range covers the comment above the method; it must not stop the walk.
    #[test]
    fn being_written_inside_something_does_not_end_it() {
        let source = "class One {\n  // what it does\n  two() {}\n}\n";
        let wholes = [
            at(source, "class One {\n  // what it does\n  two() {}\n}"),
            at(source, "two() {}"),
        ];
        assert_eq!(above(source, &wholes), ["// what it does"]);
    }

    #[test]
    fn it_stops_below_the_line_it_is_written_inside() {
        let source = "// about the class\nclass One {\n  two() {}\n}\n";
        let whole = at(source, "class One {\n  two() {}\n}");
        let wholes = [whole.clone(), at(source, "two() {}")];
        assert_eq!(above(source, &wholes), ["// about the class"]);
    }

    /// A server may report a definition from a token in the middle of the line.
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
