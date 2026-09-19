use crate::change::{Change, classify};
use crate::diagnostic::Diagnostic;
use crate::model::{Definition, Identity};
use crate::propagate::affected;
use crate::reference::{Reference, Target};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

/// One definition depending on another, as the reader will see it. Definitions that
/// nobody needs to read are left out, so an edge can stand in for a chain that ran
/// through them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Edge {
    pub from: Identity,
    pub to: Identity,
    /// Definitions the chain passed through on the way, in order. Empty when the two
    /// mention each other directly.
    pub via: Vec<Identity>,
}

/// Everything worth reading in a change, and how it hangs together.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Review {
    pub changes: BTreeMap<Identity, Change>,
    /// Didn't change, but sits downstream of something that did.
    pub affected: BTreeSet<Identity>,
    pub members: BTreeSet<Identity>,
    pub edges: Vec<Edge>,
    pub diagnostics: Vec<Diagnostic>,
}

pub fn review(definitions: &[Definition], references: &[Reference]) -> Review {
    let mut diagnostics = Vec::new();
    let changes: BTreeMap<Identity, Change> = definitions
        .iter()
        .map(|def| {
            let (change, mut found) = classify(def);
            diagnostics.append(&mut found);
            (def.identity, change)
        })
        .collect();

    let (affected, mut found) = affected(&changes, references);
    diagnostics.append(&mut found);

    let members: BTreeSet<Identity> = changes
        .iter()
        .filter(|(_, change)| change.worth_reading())
        .map(|(identity, _)| *identity)
        .chain(affected.iter().copied())
        .collect();

    let edges = project(&members, references);

    Review {
        changes,
        affected,
        members,
        edges,
        diagnostics,
    }
}

/// A mention counts if it was there in either snapshot, so that a deleted definition
/// still hangs off whatever it used to call.
fn dependencies(references: &[Reference]) -> BTreeMap<Identity, Vec<Identity>> {
    let mut out: BTreeMap<Identity, Vec<Identity>> = BTreeMap::new();
    for reference in references {
        if let Target::Known(to) = reference.to
            && reference.from != to
        {
            out.entry(reference.from).or_default().push(to);
        }
    }
    out
}

/// Walk out from each member, stepping over anything the reader won't see, so two
/// changed definitions joined by an untouched helper still look joined.
fn project(members: &BTreeSet<Identity>, references: &[Reference]) -> Vec<Edge> {
    let dependencies = dependencies(references);
    let mut edges = Vec::new();

    for from in members {
        let mut seen = BTreeSet::from([*from]);
        let mut queue: VecDeque<(Identity, Vec<Identity>)> = dependencies
            .get(from)
            .into_iter()
            .flatten()
            .map(|to| (*to, Vec::new()))
            .collect();

        while let Some((node, via)) = queue.pop_front() {
            if !seen.insert(node) {
                continue;
            }
            if members.contains(&node) {
                edges.push(Edge {
                    from: *from,
                    to: node,
                    via,
                });
                continue;
            }
            for next in dependencies.get(&node).into_iter().flatten() {
                let mut via = via.clone();
                via.push(node);
                queue.push_back((*next, via));
            }
        }
    }

    edges
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Part, Sides};
    use crate::testing::{occurrence, reference};

    fn edited(id: u32, name: &str) -> Definition {
        Definition {
            identity: Identity(id),
            sides: Sides::Kept {
                before: occurrence(name, &[(Part::Body, "old")]),
                after: occurrence(name, &[(Part::Body, "new")]),
            },
        }
    }

    fn untouched(id: u32, name: &str) -> Definition {
        Definition {
            identity: Identity(id),
            sides: Sides::Kept {
                before: occurrence(name, &[(Part::Body, "same")]),
                after: occurrence(name, &[(Part::Body, "same")]),
            },
        }
    }

    #[test]
    fn only_definitions_worth_reading_get_in() {
        let review = review(&[edited(0, "lookupPrice"), untouched(1, "withRetry")], &[]);

        assert_eq!(review.members, BTreeSet::from([Identity(0)]));
    }

    /// buildLineItems calls withRetry calls lookupPrice. The helper is untouched and
    /// generic, so the reader never sees it, but the two edits are still related.
    #[test]
    fn an_untouched_helper_is_stepped_over() {
        let definitions = [
            edited(0, "buildLineItems"),
            untouched(1, "withRetry"),
            edited(2, "lookupPrice"),
        ];
        let references = [reference(0, 1, Part::Body), reference(1, 2, Part::Body)];

        let review = review(&definitions, &references);

        assert_eq!(
            review.edges,
            vec![Edge {
                from: Identity(0),
                to: Identity(2),
                via: vec![Identity(1)],
            }]
        );
    }

    #[test]
    fn a_direct_mention_has_nothing_in_between() {
        let definitions = [edited(0, "cartTotal"), edited(1, "lineTotal")];
        let review = review(&definitions, &[reference(0, 1, Part::Body)]);

        assert_eq!(
            review.edges,
            vec![Edge {
                from: Identity(0),
                to: Identity(1),
                via: Vec::new(),
            }]
        );
    }

    #[test]
    fn a_chain_of_hidden_helpers_is_kept_in_order() {
        let definitions = [
            edited(0, "handleCheckout"),
            untouched(1, "middle"),
            untouched(2, "inner"),
            edited(3, "lookupPrice"),
        ];
        let references = [
            reference(0, 1, Part::Body),
            reference(1, 2, Part::Body),
            reference(2, 3, Part::Body),
        ];

        let review = review(&definitions, &references);

        assert_eq!(review.edges[0].via, vec![Identity(1), Identity(2)]);
    }

    #[test]
    fn a_dead_end_helper_produces_no_edge() {
        let definitions = [edited(0, "cartTotal"), untouched(1, "log")];
        let review = review(&definitions, &[reference(0, 1, Part::Body)]);

        assert!(review.edges.is_empty());
    }
}
