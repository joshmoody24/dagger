use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The pieces a definition's text is split into. A part can be missing when it
/// doesn't apply, like a type alias that has no body.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum Part {
    /// The contract. Changing it can break callers.
    Type,
    /// Internal. Changing it can't break callers.
    Body,
    /// Written for callers, but changing it can't break them.
    Docs,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct Span {
    pub start: u32,
    pub end: u32,
}

/// One stretch of source belonging to a part.
///
/// A part is made of these rather than being one of them, because the source a part
/// covers isn't always in one stretch. A module's imports can sit wherever the language
/// allows them, which in most languages is anywhere. A C function can be declared at the
/// top of a file and again further down.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct Piece {
    pub text: String,
    pub span: Span,
    /// Which line of the file this starts on, counting from one.
    ///
    /// A span is where the bytes are, which is what the machinery needs and nothing a
    /// person can use. A reader points at a line — "the check on line 31" — and only
    /// whoever read the file can say which line that is, so it's said here rather than
    /// worked out later from text nobody kept.
    pub line: u32,
    /// Which file this stretch is in.
    ///
    /// Always said, even when it's the definition's own. It used to be set only when it
    /// differed, which made one place spell itself two ways — and everything comparing two
    /// pieces had to know that, or quietly decide that a piece saying nothing and a piece
    /// naming its own file were in different files. Two bugs came of it: an overlap that
    /// went unreported, and a part that read as having moved when it hadn't.
    pub file: String,
}

/// Where a definition lives in one snapshot. Extractors have to keep this unique,
/// merging things they can't tell apart, like an overload set. The name is kept
/// apart from the scope because renaming breaks callers and moving doesn't.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct Locator {
    pub scope: Vec<String>,
    pub name: String,
}

/// Whether a definition can hold others.
///
/// Told apart because a page draws the two differently: a container is the box, and what it
/// holds are the things in it. Intrinsic, and the extractor's to say — a module is a
/// container whether or not anything inside it changed, and the empty box drawn around
/// nothing is exactly the case that needs saying out loud.
///
/// This used to be a string comparison against `"module"`, in four places across a protocol
/// boundary, which meant every language had to spell its containers that one way or go
/// undrawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// Read on its own, holds nothing.
    #[default]
    Item,
    /// Can hold other definitions. May still change, and still be worth reading.
    Container,
}

/// A definition as it exists in one snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct Occurrence {
    pub locator: Locator,
    /// Whether this holds other definitions.
    #[serde(default)]
    pub role: Role,
    /// What it's written inside, in the language's own structure: a method's impl, an
    /// impl's module, a module's module. `None` at the root.
    ///
    /// Said by whoever parsed the file, because only they know. Worked out afterwards from
    /// the scope and the kind, it comes out wrong in the ordinary cases — a method's scope
    /// names its type, not the impl block it sits in — and it costs fifty lines to be wrong
    /// in.
    #[serde(default)]
    pub parent: Option<Locator>,
    /// What the extractor calls this, like "function" or "test". Display only.
    pub kind: String,
    /// Where the definition lives, and where its parts live unless they say otherwise.
    pub file: String,
    /// The source, split up for the reader. Only used for display and for checking that
    /// every changed byte belongs somewhere. Each part's pieces are in the order they
    /// appear, which is the order a reader would meet them.
    pub parts: BTreeMap<Part, Vec<Piece>>,
    /// How the definition looks from outside, according to the compiler. Not found
    /// anywhere in the source, which is why it sits apart from the parts.
    ///
    /// This decides whether callers broke, so when it's here it beats the type part,
    /// and a change to an inferred return type can't pass as a body change. An
    /// extractor that supplies it can dump the whole definition into one part and
    /// still get every downstream answer right. It just won't read as nicely.
    pub contract: Option<String>,
}

impl Occurrence {
    /// The part's text, its pieces run together. What a reader would see if the stretches
    /// were laid end to end, and what comparing two sides comes down to.
    pub fn text_of(&self, part: Part) -> Option<String> {
        let pieces = self.parts.get(&part)?;
        Some(
            pieces
                .iter()
                .map(|piece| piece.text.as_str())
                .collect::<Vec<_>>()
                .join("\n"),
        )
    }

    /// Which files a part is spread across. More than one only where a language lets a
    /// definition be split, the way C puts a declaration in a header.
    pub fn files_of(&self, part: Part) -> Vec<&str> {
        let mut files: Vec<&str> = self
            .parts
            .get(&part)
            .into_iter()
            .flatten()
            .map(|piece| piece.file.as_str())
            .collect();
        /* Sorted before the duplicates come out, since `dedup` only drops the ones next to
         * each other. A part whose pieces sit in one file, then another, then the first
         * again came back naming three files, and two of them the same — which reads as a
         * part that moved when nothing moved at all. */
        files.sort_unstable();
        files.dedup();
        files
    }

    /// Every piece of every part, in the order they appear, with the file each sits in.
    pub fn pieces(&self) -> Vec<(&str, &Piece)> {
        let mut pieces: Vec<(&str, &Piece)> = self
            .parts
            .values()
            .flatten()
            .map(|piece| (piece.file.as_str(), piece))
            .collect();
        pieces.sort_by_key(|(file, piece)| (*file, piece.span.start));
        pieces
    }
}

/// Handed out by matching. Means nothing on its own.
///
/// Written as a string, because that's what it becomes. JSON has no number for an object
/// key, so every one of these used to arrive at the page spelled differently depending on
/// whether it was a key or a value — and the page turned the keys back into numbers to
/// match. A handle that means nothing may as well be the shape it travels in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS), ts(type = "string"))]
pub struct Identity(pub u32);

impl Serialize for Identity {
    fn serialize<S: serde::Serializer>(&self, out: S) -> Result<S::Ok, S::Error> {
        out.serialize_str(&self.0.to_string())
    }
}

impl<'de> Deserialize<'de> for Identity {
    fn deserialize<D: serde::Deserializer<'de>>(from: D) -> Result<Self, D::Error> {
        let said = String::deserialize(from)?;
        said.parse().map(Identity).map_err(serde::de::Error::custom)
    }
}

/// Which snapshots a definition showed up in. Being missing on one side is the only
/// thing that makes adding and removing different from any other change.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum Sides {
    Added(Occurrence),
    Removed(Occurrence),
    Kept {
        before: Occurrence,
        after: Occurrence,
    },
}

impl Sides {
    pub fn before(&self) -> Option<&Occurrence> {
        match self {
            Sides::Added(_) => None,
            Sides::Removed(occ) | Sides::Kept { before: occ, .. } => Some(occ),
        }
    }

    pub fn after(&self) -> Option<&Occurrence> {
        match self {
            Sides::Removed(_) => None,
            Sides::Added(occ) | Sides::Kept { after: occ, .. } => Some(occ),
        }
    }

    /// The newest version we have, for anything that just needs a name to show.
    pub fn latest(&self) -> &Occurrence {
        match self {
            Sides::Added(occ) | Sides::Removed(occ) | Sides::Kept { after: occ, .. } => occ,
        }
    }
}

/// One definition across both snapshots.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct Definition {
    pub identity: Identity,
    pub sides: Sides,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(file: &str, start: u32, end: u32) -> Piece {
        Piece {
            text: String::new(),
            span: Span { start, end },
            line: 1,
            file: file.to_string(),
        }
    }

    fn spread(pieces: Vec<Piece>) -> Occurrence {
        Occurrence {
            locator: Locator {
                scope: Vec::new(),
                name: "one".to_string(),
            },
            role: Role::Item,
            parent: None,
            kind: "function".to_string(),
            file: "own.c".to_string(),
            parts: BTreeMap::from([(Part::Type, pieces)]),
            contract: None,
        }
    }

    /* A part that leaves its own file and comes back sits in two files, not three. Counted
     * as three, comparing one side against the other says it moved when nothing did. */
    #[test]
    fn a_part_that_returns_to_a_file_is_not_in_it_twice() {
        let occurrence = spread(vec![
            at("own.c", 0, 1),
            at("other.h", 0, 1),
            at("own.c", 2, 3),
        ]);
        assert_eq!(occurrence.files_of(Part::Type), ["other.h", "own.c"]);
    }
}
