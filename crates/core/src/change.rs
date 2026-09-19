use crate::model::{Definition, Occurrence, Part};
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

/// The compiler's word on how a definition looks from outside, falling back to the
/// signature as written when no extractor could supply one.
fn contracts<'a>(
    before: &'a Occurrence,
    after: &'a Occurrence,
) -> (Option<&'a str>, Option<&'a str>) {
    match (before.contract.as_deref(), after.contract.as_deref()) {
        (Some(b), Some(a)) => (Some(b), Some(a)),
        _ => (part_text(before, Part::Type), part_text(after, Part::Type)),
    }
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

pub fn classify(def: &Definition) -> Change {
    match (&def.before, &def.after) {
        (None, Some(_)) => Change::Added,
        (Some(_), None) => Change::Removed,
        (Some(before), Some(after)) => {
            let (before_contract, after_contract) = contracts(before, after);
            Change::Kept(Edits {
                contract: before_contract != after_contract
                    || before.locator.name != after.locator.name,
                moved: before.file != after.file || before.locator.scope != after.locator.scope,
                parts: changed_parts(before, after),
            })
        }
        (None, None) => unreachable!("a definition exists in at least one snapshot"),
    }
}
