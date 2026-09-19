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

/// Whether callers see something different. Any of three signals is enough: the name,
/// the signature as written, or what the extractor's tooling says the thing looks like
/// from outside.
///
/// Taking whichever fires rather than letting one overrule another is deliberate. A
/// contract that came from a summary — an editor's hover text, say — can be identical on
/// both sides while the declaration plainly changed, and letting that silence the written
/// text would hide a real break. Reading a definition that turned out to be fine costs a
/// moment; missing one that broke costs more.
fn contract_changed(before: &Occurrence, after: &Occurrence) -> bool {
    let told = match (before.contract.as_deref(), after.contract.as_deref()) {
        (Some(before), Some(after)) => before != after,
        _ => false,
    };

    told || part_text(before, Part::Type) != part_text(after, Part::Type)
        || before.locator.name != after.locator.name
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

    /// A signature reflowed by a formatter, which callers don't care about. We say they
    /// might anyway: the alternative is trusting a contract that can be a summary, and a
    /// summary that stays the same while the declaration changes would hide a real break.
    #[test]
    fn a_rewritten_signature_counts_even_when_the_contract_agrees() {
        let mut before = occurrence("parseId", &[(Part::Type, "parseId(s: string)")]);
        before.contract = Some("parseId(s: string): number".to_string());
        let mut after = occurrence("parseId", &[(Part::Type, "parseId(\n  s: string,\n)")]);
        after.contract = Some("parseId(s: string): number".to_string());
        let edits = edits(Sides::Kept { before, after });

        assert!(edits.contract);
        assert!(edits.changed(Part::Type));
    }

    /// The case the union exists for: a contract that summarises rather than spells out,
    /// so only the written declaration shows the member arriving.
    #[test]
    fn a_summarising_contract_cannot_hide_a_changed_declaration() {
        let mut before = occurrence(
            "Money",
            &[(Part::Type, "interface Money { amount: number }")],
        );
        before.contract = Some("interface Money".to_string());
        let mut after = occurrence(
            "Money",
            &[(
                Part::Type,
                "interface Money { amount: number; precise: boolean }",
            )],
        );
        after.contract = Some("interface Money".to_string());

        assert!(edits(Sides::Kept { before, after }).contract);
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
