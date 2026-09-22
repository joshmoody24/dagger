use crate::model::{Identity, Locator, Part, Span};
use serde::{Deserialize, Serialize};

/// Which extractor claimed a reference, like "tsc" or "http-route". Kept around so the
/// reader can tell a compiler's answer from a guess.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ExtractorId(pub String);

/// What a mention points at. Some mentions never bind, like a dynamic call, and we
/// keep those instead of dropping them so the reader knows we came up empty.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Target<T> {
    Known(T),
    Unknown { symbol: String },
}

/// One spot in the source where a definition mentions another.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Site {
    pub part: Part,
    pub span: Span,
    pub extractor: ExtractorId,
}

/// A mention found in a single snapshot. Binding a name to a declaration is a
/// question for the compiler, which only ever sees one snapshot, so this is as far
/// as an extractor can get.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mention {
    pub from: Locator,
    pub to: Target<Locator>,
    pub site: Site,
}

/// The two snapshots' mentions lined up, once matching has handed out identities.
/// An empty side means the mention wasn't there, so a call that came or went reads
/// the same way as a definition that came or went.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reference {
    pub from: Identity,
    pub to: Target<Identity>,
    pub before: Vec<Site>,
    pub after: Vec<Site>,
}
