use crate::diagnostic::Diagnostic;
use crate::model::{Definition, Occurrence, Part, Sides};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// What happened to one definition between the two snapshots.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Change {
    Added,
    Removed,
    Kept(Edits),
}

/// What differs about a definition that stuck around. An empty one means it sat still.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Edits {
    /// How it looks to callers, including its name.
    pub contract: bool,
    /// Landed in a different file or scope. Nobody breaks over this, so on its own
    /// it isn't worth reviewing.
    pub moved: bool,
    /// The parts whose text differs.
    pub parts: BTreeSet<Part>,
}

impl Edits {
    pub fn changed(&self, part: Part) -> bool {
        self.parts.contains(&part)
    }

    /// Worth putting in front of a reader. A pure move isn't.
    pub fn worth_reading(&self) -> bool {
        self.contract || !self.parts.is_empty()
    }
}

impl Change {
    /// Whether callers of this definition have to change too.
    pub fn breaks_callers(&self) -> bool {
        match self {
            Change::Added => false,
            Change::Removed => true,
            Change::Kept(edits) => edits.contract,
        }
    }

    pub fn worth_reading(&self) -> bool {
        match self {
            Change::Added | Change::Removed => true,
            Change::Kept(edits) => edits.worth_reading(),
        }
    }
}

fn part_text(occ: &Occurrence, part: Part) -> Option<&str> {
    occ.parts.get(&part).map(|p| p.text.as_str())
}

fn changed_parts(before: &Occurrence, after: &Occurrence) -> BTreeSet<Part> {
    before
        .parts
        .keys()
        .chain(after.parts.keys())
        .copied()
        .filter(|&part| part_text(before, part) != part_text(after, part))
        .collect()
}

/// Whether callers see something different, preferring the compiler's view of the
/// definition and falling back to the signature as written.
fn contract_changed(before: &Occurrence, after: &Occurrence) -> bool {
    let text = match (before.contract.as_deref(), after.contract.as_deref()) {
        (Some(b), Some(a)) => b != a,
        _ => part_text(before, Part::Type) != part_text(after, Part::Type),
    };
    text || before.locator.name != after.locator.name
}

pub fn classify(def: &Definition) -> (Change, Vec<Diagnostic>) {
    match &def.sides {
        Sides::Added(_) => (Change::Added, Vec::new()),
        Sides::Removed(_) => (Change::Removed, Vec::new()),
        Sides::Kept { before, after } => {
            let lopsided = before.contract.is_some() != after.contract.is_some();
            let change = Change::Kept(Edits {
                contract: contract_changed(before, after),
                moved: before.file != after.file || before.locator.scope != after.locator.scope,
                parts: changed_parts(before, after),
            });
            let diagnostics = if lopsided {
                vec![Diagnostic::LopsidedContract {
                    definition: def.identity,
                }]
            } else {
                Vec::new()
            };
            (change, diagnostics)
        }
    }
}
