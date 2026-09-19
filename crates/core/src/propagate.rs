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
