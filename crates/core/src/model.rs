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
    /// Set when this piece lives somewhere other than the definition's own file, the
    /// way a C declaration sits in a header away from its body.
    pub file: Option<String>,
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

/// A definition as it exists in one snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct Occurrence {
    pub locator: Locator,
    /// What the extractor calls this, like "function" or "test". We never read it.
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
            .map(|piece| piece.file.as_deref().unwrap_or(&self.file))
            .collect();
        files.dedup();
        files
    }

    /// Every piece of every part, in the order they appear, with the file each sits in.
    pub fn pieces(&self) -> Vec<(&str, &Piece)> {
        let mut pieces: Vec<(&str, &Piece)> = self
            .parts
            .values()
            .flatten()
            .map(|piece| (piece.file.as_deref().unwrap_or(&self.file), piece))
            .collect();
        pieces.sort_by_key(|(file, piece)| (*file, piece.span.start));
        pieces
    }
}

/// Handed out by matching. Means nothing on its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct Identity(pub u32);

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
