use crate::model::Identity;
use serde::{Deserialize, Serialize};

/// Something an adapter handed us that we had to work around. We answer anyway, but
/// the answer came from worse information than it should have, and the reader
/// deserves to hear about it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Diagnostic {
    /// One snapshot had a compiler's view of this definition and the other didn't,
    /// so we fell back to comparing the signature as written. Usually means the file
    /// changed language or stopped type checking.
    LopsidedContract { definition: Identity },
}
