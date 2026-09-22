use crate::model::{Identity, Locator};
use serde::{Deserialize, Serialize};

/// Something an adapter handed us that we had to work around. We answer anyway, but
/// the answer came from worse information than it should have, and the reader
/// deserves to hear about it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum Diagnostic {
    /// One snapshot had a compiler's view of this definition and the other didn't,
    /// so we fell back to comparing the signature as written. Usually means the file
    /// changed language or stopped type checking.
    LopsidedContract { definition: Identity },
    /// A name in this definition's type part that no binder could place. If it turns
    /// out to be something that changed, we missed telling the reader about it.
    UnboundInContract {
        definition: Identity,
        symbol: String,
    },
    /// An extractor reported a mention coming from a definition it never reported.
    MentionFromNowhere { from: Locator },
    /// A definition whose own pieces overlap, or run backwards.
    ///
    /// Pieces are the definition cut into parts, so they divide it up rather than covering
    /// each other. Two pieces over the same text means the same line is shown twice — and
    /// when they're in different parts, that one line is both contract and prose, so a
    /// comment change reads as breaking every caller or the other way about.
    Tangled {
        definition: Locator,
        /// Where the trouble starts, in bytes.
        at: u32,
    },
    /// Two definitions in one snapshot answering to the same name.
    ///
    /// A definition has to be addressable by name, or it can't be told from its twin in the
    /// other snapshot. One of them gets matched and the rest read as arriving or leaving,
    /// which nobody did — so this can invent a change as easily as hide one.
    TwoOfOneName { locator: Locator, times: usize },
    /// Lines that differ between the snapshots but sit inside no definition, so nothing in
    /// the review accounts for them. Whatever an extractor walked past. A reader who trusts
    /// the review would never learn these changed.
    Unattributed {
        file: String,
        lines: usize,
        /// The first few, so this can be looked into rather than just counted.
        at: Vec<u32>,
    },
}

impl Diagnostic {
    /// Which definition this is about, where it's about one that has an identity.
    ///
    /// Some aren't: lines belonging to nothing are about a file, and a name reported twice
    /// is about a name that couldn't become an identity in the first place.
    pub fn about(&self) -> Option<Identity> {
        match self {
            Diagnostic::LopsidedContract { definition }
            | Diagnostic::UnboundInContract { definition, .. } => Some(*definition),
            Diagnostic::MentionFromNowhere { .. }
            | Diagnostic::Tangled { .. }
            | Diagnostic::TwoOfOneName { .. }
            | Diagnostic::Unattributed { .. } => None,
        }
    }

    /// Whether this one means the review might not be showing something that changed.
    ///
    /// These aren't all the same kind of bad news, and reporting them as though they were
    /// teaches a reader to ignore the lot. Changed lines nobody accounts for, a mention
    /// from a definition that was never reported, a name in a contract that nothing could
    /// place — each of those can hide a change, and a reader trusting the review would
    /// never learn of it. A lopsided contract is different in kind: the change is there
    /// and it's shown, it was just worked out from the text rather than the compiler.
    pub fn hides(&self) -> bool {
        !matches!(self, Diagnostic::LopsidedContract { .. })
    }
}
