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

/// One stretch of source belonging to a part. A part is a list of these because its
/// source isn't always contiguous: imports can sit anywhere, and a C function can be
/// declared in a header and defined elsewhere.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct Piece {
    pub text: String,
    pub span: Span,
    /// Line this starts on, from one. Spans are byte offsets, which readers can't use, and
    /// only the extractor that read the file knows the line.
    pub line: u32,
    /// Always set, even when it's the definition's own file, so two pieces can be compared
    /// without knowing about a default.
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

/// Whether a definition can hold others. The page draws containers as boxes. The
/// extractor says which is which, since a module is a container even if nothing inside
/// it changed.
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
    /// What it's written inside: a method's impl, an impl's module. `None` at the root.
    /// Set by the extractor, because deriving it from scope and kind gets ordinary cases
    /// wrong (a method's scope names its type, not its impl block).
    #[serde(default)]
    pub parent: Option<Locator>,
    /// What the extractor calls this, like "function" or "test". Display only.
    pub kind: String,
    /// Where the definition lives, and where its parts live unless they say otherwise.
    pub file: String,
    /// The source, split up for display and for checking that every changed byte belongs
    /// somewhere. Each part's pieces are in source order.
    pub parts: BTreeMap<Part, Vec<Piece>>,
    /// The compiler's view of the definition from outside; not in the source, so kept
    /// apart from the parts. When present it overrides the type part for deciding whether
    /// callers broke, so an inferred return type change can't pass as a body change.
    pub contract: Option<String>,
}

impl Occurrence {
    /// The part's text, its pieces joined.
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
        // `dedup` only drops adjacent duplicates.
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

/// Handed out by matching. Means nothing on its own. Serialized as a string, since JSON
/// object keys are strings and the page shouldn't have to convert between the two.
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

    /// A part that leaves its file and comes back is in two files, not three.
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
