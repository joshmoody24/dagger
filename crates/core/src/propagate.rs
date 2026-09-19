use crate::change::Change;
use crate::diagnostic::Diagnostic;
use crate::model::{Identity, Part};
use crate::reference::{Reference, Target};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

/// Definitions that didn't change but sit downstream of something that did, reached
/// by following type parts only. A body can call whatever it likes without its own
/// callers caring, so the trail stops at the first body.
pub fn affected(
    changes: &BTreeMap<Identity, Change>,
    references: &[Reference],
) -> (BTreeSet<Identity>, Vec<Diagnostic>) {
    let mut callers: BTreeMap<Identity, Vec<Identity>> = BTreeMap::new();
    let mut diagnostics = Vec::new();

    for reference in references.iter().filter(|r| mentioned_in_type(r)) {
        match &reference.to {
            Target::Known(to) => callers.entry(*to).or_default().push(reference.from),
            Target::Unknown { symbol } => diagnostics.push(Diagnostic::UnboundInContract {
                definition: reference.from,
                symbol: symbol.clone(),
            }),
        }
    }

    let mut reached = BTreeSet::new();
    let mut queue: VecDeque<Identity> = changes
        .iter()
        .filter(|(_, change)| change.breaks_callers())
        .map(|(identity, _)| *identity)
        .collect();

    while let Some(broken) = queue.pop_front() {
        for caller in callers.get(&broken).into_iter().flatten() {
            if reached.insert(*caller) {
                queue.push_back(*caller);
            }
        }
    }

    (reached, diagnostics)
}

/// Whether the mention still stands in the newer snapshot, somewhere its own callers
/// can see. A call that was deleted breaks nobody.
fn mentioned_in_type(reference: &Reference) -> bool {
    reference.after.iter().any(|site| site.part == Part::Type)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::change::Edits;
    use crate::testing::reference;

    /// A contract break at `0`, and nothing else changed.
    fn broken_at_zero() -> BTreeMap<Identity, Change> {
        BTreeMap::from([(
            Identity(0),
            Change::Kept(Edits {
                contract: true,
                ..Edits::default()
            }),
        )])
    }

    fn reached(references: &[Reference]) -> BTreeSet<Identity> {
        affected(&broken_at_zero(), references).0
    }

    #[test]
    fn a_type_reference_carries_the_break_along() {
        let references = [
            reference(1, 0, Part::Type),
            reference(2, 1, Part::Type),
            reference(3, 2, Part::Type),
        ];

        assert_eq!(
            reached(&references),
            BTreeSet::from([Identity(1), Identity(2), Identity(3)])
        );
    }

    #[test]
    fn a_body_reference_stops_the_trail() {
        let references = [reference(1, 0, Part::Body), reference(2, 1, Part::Type)];

        assert!(reached(&references).is_empty());
    }

    /// `1` still gets hit, but nothing rides along behind it.
    #[test]
    fn the_trail_stops_at_the_first_body() {
        let references = [reference(1, 0, Part::Type), reference(2, 1, Part::Body)];

        assert_eq!(reached(&references), BTreeSet::from([Identity(1)]));
    }

    #[test]
    fn a_mention_that_was_deleted_breaks_nobody() {
        let mut deleted = reference(1, 0, Part::Type);
        deleted.after.clear();

        assert!(reached(&[deleted]).is_empty());
    }

    #[test]
    fn nothing_is_affected_when_nothing_breaks() {
        let untouched = BTreeMap::from([(Identity(0), Change::Kept(Edits::default()))]);
        let references = [reference(1, 0, Part::Type)];

        assert!(affected(&untouched, &references).0.is_empty());
    }

    #[test]
    fn a_cycle_settles() {
        let references = [reference(1, 0, Part::Type), reference(0, 1, Part::Type)];

        assert_eq!(
            reached(&references),
            BTreeSet::from([Identity(0), Identity(1)])
        );
    }

    #[test]
    fn a_name_we_could_not_place_is_reported() {
        let mut unbound = reference(1, 0, Part::Type);
        unbound.to = Target::Unknown {
            symbol: "Money".to_string(),
        };

        let (_, diagnostics) = affected(&broken_at_zero(), &[unbound]);
        assert_eq!(
            diagnostics,
            vec![Diagnostic::UnboundInContract {
                definition: Identity(1),
                symbol: "Money".to_string()
            }]
        );
    }
}
