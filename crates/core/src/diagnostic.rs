use crate::model::{Identity, Locator};
use serde::{Deserialize, Serialize};

/// Something an adapter handed us that we had to work around. We still answer, but from
/// worse information than we should have, and the reader should know.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum Diagnostic {
    /// One snapshot had a compiler's view of this definition and the other didn't, so we
    /// fell back to comparing the signature as written.
    LopsidedType { definition: Identity },
    /// A name in this definition's type part that no extractor could place. If it turns
    /// out to be something that changed, we missed telling the reader about it.
    UnboundInType {
        definition: Identity,
        symbol: String,
    },
    /// An extractor reported a mention coming from a definition it never reported.
    MentionFromNowhere { from: Locator },
    /// A definition whose own pieces overlap, or run backwards. Overlapping pieces show
    /// the same line twice, and across parts make one line both type and prose.
    Tangled {
        definition: Locator,
        /// Where the trouble starts, in bytes.
        at: u32,
    },
    /// Two definitions in one snapshot with the same name. Only one gets matched and the
    /// rest read as added or removed, so this can invent a change or hide one.
    TwoOfOneName { locator: Locator, times: usize },
    /// Lines that differ between the snapshots but sit inside no definition, so the review
    /// never shows them.
    Unattributed {
        file: String,
        lines: usize,
        /// The first few, so this can be looked into rather than just counted.
        at: Vec<u32>,
    },
}

impl Diagnostic {
    /// Which definition this is about, when it's about one with an identity.
    pub fn about(&self) -> Option<Identity> {
        match self {
            Diagnostic::LopsidedType { definition }
            | Diagnostic::UnboundInType { definition, .. } => Some(*definition),
            Diagnostic::MentionFromNowhere { .. }
            | Diagnostic::Tangled { .. }
            | Diagnostic::TwoOfOneName { .. }
            | Diagnostic::Unattributed { .. } => None,
        }
    }

    /// Whether this could mean the review is missing a change. A lopsided type can't:
    /// the change is shown, just worked out from the text rather than the compiler.
    pub fn hides(&self) -> bool {
        !matches!(self, Diagnostic::LopsidedType { .. })
    }
}
