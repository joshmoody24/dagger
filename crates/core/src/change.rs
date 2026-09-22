use crate::diagnostic::Diagnostic;
use crate::model::{Definition, Occurrence, Part, Sides};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// What happened to one definition between the two snapshots.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum Change {
    Added,
    Removed,
    Kept(Edits),
}

/// What differs about a definition that stuck around. An empty one means it sat still.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
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

fn part_text(occ: &Occurrence, part: Part) -> Option<String> {
    occ.text_of(part)
}

/// Landed somewhere else. A part moving on its own counts too, the way a C declaration
/// can migrate to a different header.
fn moved(before: &Occurrence, after: &Occurrence) -> bool {
    let part_moved = before.parts.keys().chain(after.parts.keys()).any(|&part| {
        let (was, is) = (before.files_of(part), after.files_of(part));
        !was.is_empty() && !is.is_empty() && was != is
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

/// Whether callers see something different: the name, the written signature, or the
/// tooling's contract. Any one is enough. The contract can be a summary (hover text) that
/// stays the same while the declaration changed, so it must not overrule the written text.
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
    use crate::testing::{occurrence, piece};

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
        after.parts.get_mut(&Part::Type).unwrap()[0].file = "money.h".to_string();
        let edits = edits(Sides::Kept { before, after });

        assert!(edits.moved);
        assert!(!edits.worth_reading());
    }

    /// A module's imports are rarely one stretch; editing the second must count like the first.
    #[test]
    fn a_part_made_of_several_pieces_is_compared_as_a_whole() {
        let mut before = occurrence("money", &[(Part::Body, "use std::fmt;")]);
        before
            .parts
            .get_mut(&Part::Body)
            .unwrap()
            .push(piece("use std::io;"));

        let mut after = occurrence("money", &[(Part::Body, "use std::fmt;")]);
        after
            .parts
            .get_mut(&Part::Body)
            .unwrap()
            .push(piece("use std::net;"));

        assert!(edits(Sides::Kept { before, after }).changed(Part::Body));
    }

    /// A declaration in a header and again in its source file: one part, two files.
    #[test]
    fn a_part_can_sit_in_two_files_without_having_moved() {
        let split = || {
            let mut occurrence = occurrence("helper", &[(Part::Type, "int helper(void);")]);
            let pieces = occurrence.parts.get_mut(&Part::Type).unwrap();
            pieces[0].file = "money.h".to_string();
            pieces.push(piece("int helper(void)"));
            occurrence
        };

        let edits = edits(Sides::Kept {
            before: split(),
            after: split(),
        });
        assert!(!edits.moved);
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

    /// A reflowed signature counts because the contract may be a summary that hides real breaks.
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
