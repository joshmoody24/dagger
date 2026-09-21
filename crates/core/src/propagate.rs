use crate::change::Change;
use crate::diagnostic::Diagnostic;
use crate::model::{Identity, Part};
use crate::reference::{Reference, Target};
use std::collections::{BTreeMap, VecDeque};

/// Definitions that didn't change but sit downstream of something that did, reached
/// by following type parts only. A body can call whatever it likes without its own
/// callers caring, so the trail stops at the first body.
///
/// Each comes back with how many hops away it is, because that's the thing a reader wants
/// to turn down: the definitions that call a change directly are usually worth seeing, and
/// the ones five removes away are the same news arriving for the fifth time. Told the
/// distance, a page can show as much of it as somebody asks for.
///
/// `depth` is how far to follow. Nought means don't: what changed is the whole review.
pub fn affected(
    changes: &BTreeMap<Identity, Change>,
    references: &[Reference],
    depth: u32,
) -> (BTreeMap<Identity, u32>, Vec<Diagnostic>) {
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

    let mut reached: BTreeMap<Identity, u32> = BTreeMap::new();
    let mut queue: VecDeque<(Identity, u32)> = changes
        .iter()
        .filter(|(_, change)| change.breaks_callers())
        .map(|(identity, _)| (*identity, 0))
        .collect();

    /* Breadth first, so the first time a definition is reached is by the shortest way to
     * it. Reached again further round, it keeps the nearer distance — which is the honest
     * one: something both called directly and called again through a chain is, to a
     * reader, called directly. */
    while let Some((broken, away)) = queue.pop_front() {
        if away >= depth {
            continue;
        }
        for caller in callers.get(&broken).into_iter().flatten() {
            if let std::collections::btree_map::Entry::Vacant(spot) = reached.entry(*caller) {
                spot.insert(away + 1);
                queue.push_back((*caller, away + 1));
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
    use std::collections::BTreeSet;

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

    /// As far as it goes, which is what these are about unless they say otherwise.
    fn reached(references: &[Reference]) -> BTreeSet<Identity> {
        affected(&broken_at_zero(), references, u32::MAX)
            .0
            .into_keys()
            .collect()
    }

    /// How far out each one sits.
    fn away(references: &[Reference], depth: u32) -> BTreeMap<Identity, u32> {
        affected(&broken_at_zero(), references, depth).0
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

        assert!(affected(&untouched, &references, u32::MAX).0.is_empty());
    }

    /* Round and back again without going round again. */
    #[test]
    fn a_cycle_settles() {
        let references = [reference(1, 0, Part::Type), reference(0, 1, Part::Type)];

        assert_eq!(
            reached(&references),
            BTreeSet::from([Identity(0), Identity(1)])
        );
    }

    /* The distance is what a reader turns down, so it has to be the honest one. */
    #[test]
    fn each_one_says_how_far_out_it_sits() {
        let references = [
            reference(1, 0, Part::Type),
            reference(2, 1, Part::Type),
            reference(3, 2, Part::Type),
        ];

        assert_eq!(
            away(&references, u32::MAX),
            BTreeMap::from([(Identity(1), 1), (Identity(2), 2), (Identity(3), 3)])
        );
    }

    #[test]
    fn following_no_distance_at_all_reaches_nobody() {
        let references = [reference(1, 0, Part::Type)];

        assert!(away(&references, 0).is_empty());
    }

    #[test]
    fn the_trail_stops_where_it_was_told_to() {
        let references = [
            reference(1, 0, Part::Type),
            reference(2, 1, Part::Type),
            reference(3, 2, Part::Type),
        ];

        assert_eq!(away(&references, 1), BTreeMap::from([(Identity(1), 1)]));
        assert_eq!(
            away(&references, 2),
            BTreeMap::from([(Identity(1), 1), (Identity(2), 2)])
        );
    }

    /* Reached two ways, something is as near as the nearest way to it: a reader who asks
     * for what calls the change directly wants this, whatever else also leads there. */
    #[test]
    fn the_shortest_way_is_the_one_that_counts() {
        let references = [
            reference(1, 0, Part::Type),
            reference(2, 1, Part::Type),
            reference(2, 0, Part::Type),
        ];

        assert_eq!(away(&references, u32::MAX)[&Identity(2)], 1);
    }

    #[test]
    fn a_name_we_could_not_place_is_reported() {
        let mut unbound = reference(1, 0, Part::Type);
        unbound.to = Target::Unknown {
            symbol: "Money".to_string(),
        };

        let (_, diagnostics) = affected(&broken_at_zero(), &[unbound], u32::MAX);
        assert_eq!(
            diagnostics,
            vec![Diagnostic::UnboundInContract {
                definition: Identity(1),
                symbol: "Money".to_string()
            }]
        );
    }
}
