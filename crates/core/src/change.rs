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

/// Landed somewhere else, counting a part that moved out on its own the way a C
/// declaration can migrate to a different header.
fn moved(before: &Occurrence, after: &Occurrence) -> bool {
    let part_moved = before.parts.keys().chain(after.parts.keys()).any(|&part| {
        match (before.file_of(part), after.file_of(part)) {
            (Some(before), Some(after)) => before != after,
            _ => false,
        }
    });

    before.file != after.file || before.locator.scope != after.locator.scope || part_moved
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
                moved: moved(before, after),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Identity;
    use crate::testing::occurrence;

    fn classify_sides(sides: Sides) -> (Change, Vec<Diagnostic>) {
        classify(&Definition {
            identity: Identity(0),
            sides,
        })
    }

    fn edits(sides: Sides) -> Edits {
        match classify_sides(sides).0 {
            Change::Kept(edits) => edits,
            other => panic!("expected a kept definition, got {other:?}"),
        }
    }

    #[test]
    fn presence_decides_added_and_removed() {
        let occ = occurrence("zero", &[(Part::Type, "sig")]);
        assert_eq!(classify_sides(Sides::Added(occ.clone())).0, Change::Added);
        assert_eq!(classify_sides(Sides::Removed(occ)).0, Change::Removed);
    }

    #[test]
    fn a_body_edit_leaves_the_contract_alone() {
        let before = occurrence("addMoney", &[(Part::Type, "sig"), (Part::Body, "a + b")]);
        let after = occurrence(
            "addMoney",
            &[(Part::Type, "sig"), (Part::Body, "add(a, b)")],
        );
        let edits = edits(Sides::Kept { before, after });

        assert!(!edits.contract);
        assert!(edits.changed(Part::Body));
        assert!(!edits.changed(Part::Type));
        assert!(edits.worth_reading());
    }

    #[test]
    fn a_rename_breaks_callers() {
        let before = occurrence("addMoney", &[(Part::Type, "sig")]);
        let after = occurrence("plusMoney", &[(Part::Type, "sig")]);

        assert!(edits(Sides::Kept { before, after }).contract);
    }

    /// A C declaration migrating to a different header, body left where it was.
    #[test]
    fn a_part_can_move_on_its_own() {
        let before = occurrence("zero", &[(Part::Type, "sig")]);
        let mut after = occurrence("zero", &[(Part::Type, "sig")]);
        after.parts.get_mut(&Part::Type).unwrap().file = Some("money.h".to_string());
        let edits = edits(Sides::Kept { before, after });

        assert!(edits.moved);
        assert!(!edits.worth_reading());
    }

    /// A body showing up where there wasn't one isn't a part changing address.
    #[test]
    fn gaining_a_part_is_not_a_move() {
        let before = occurrence("zero", &[(Part::Type, "sig")]);
        let after = occurrence("zero", &[(Part::Type, "sig"), (Part::Body, "return 0")]);

        assert!(!edits(Sides::Kept { before, after }).moved);
    }

    #[test]
    fn moving_is_not_worth_reading_on_its_own() {
        let before = occurrence("zero", &[(Part::Type, "sig")]);
        let mut after = occurrence("zero", &[(Part::Type, "sig")]);
        after.file = "amount.ts".to_string();
        let edits = edits(Sides::Kept { before, after });

        assert!(edits.moved);
        assert!(!edits.worth_reading());
    }

    /// The signature is untouched, but the compiler says callers see something else.
    #[test]
    fn an_inferred_return_type_counts_as_a_contract_change() {
        let mut before = occurrence("parseId", &[(Part::Type, "sig"), (Part::Body, "Number(s)")]);
        before.contract = Some("parseId(s: string): number".to_string());
        let mut after = occurrence("parseId", &[(Part::Type, "sig"), (Part::Body, "s.trim()")]);
        after.contract = Some("parseId(s: string): string".to_string());
        let edits = edits(Sides::Kept { before, after });

        assert!(edits.contract);
        assert!(!edits.changed(Part::Type));
    }

    /// The reverse: a signature reflowed by a formatter, with callers unaffected.
    #[test]
    fn the_contract_wins_over_the_signature_text() {
        let mut before = occurrence("parseId", &[(Part::Type, "parseId(s: string)")]);
        before.contract = Some("parseId(s: string): number".to_string());
        let mut after = occurrence("parseId", &[(Part::Type, "parseId(\n  s: string,\n)")]);
        after.contract = Some("parseId(s: string): number".to_string());
        let edits = edits(Sides::Kept { before, after });

        assert!(!edits.contract);
        assert!(edits.changed(Part::Type));
    }

    #[test]
    fn a_contract_on_only_one_side_is_reported() {
        let mut before = occurrence("parseId", &[(Part::Type, "sig")]);
        before.contract = Some("parseId(s: string): number".to_string());
        let after = occurrence("parseId", &[(Part::Type, "sig")]);

        let (_, diagnostics) = classify_sides(Sides::Kept { before, after });
        assert_eq!(
            diagnostics,
            vec![Diagnostic::LopsidedContract {
                definition: Identity(0)
            }]
        );
    }
}
