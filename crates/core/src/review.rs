//! Everything one review of a repository comes to.
//!
//! One object, built in one place at the end, with everything about a definition on that
//! definition, so nothing can name a definition that wasn't handed over.

use crate::change::{Change, Mark, classify};
use crate::diagnostic::Diagnostic;
use crate::group::Grouping;
use crate::model::{self, Identity, Locator, Role, Sides};
use crate::order::{Cost, Step, order};
use crate::propagate::reached;
use crate::reference::{Reference, Target};
use crate::shape::{Group, Shape};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::num::NonZeroU32;

/// One definition depending on another, as the reader will see it. Definitions that
/// nobody needs to read are left out, so an edge can stand in for a chain that ran
/// through them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct Edge {
    pub from: Identity,
    pub to: Identity,
}

/// A dependency between two of `places`' members, as their places. Nothing when either
/// end isn't among them, or both ends are the same one.
pub(crate) fn placed(
    places: &BTreeMap<Identity, usize>,
    from: Identity,
    to: Identity,
) -> Option<(usize, usize)> {
    places
        .get(&from)
        .zip(places.get(&to))
        .map(|(&from, &to)| (from, to))
        .filter(|(from, to)| from != to)
}

/// How much trust a warning costs the review it's about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum Impact {
    /// Something that changed might not be here at all, and a reader trusting the review
    /// would never learn of it.
    Incomplete,
    /// It's here and it's shown, but it was worked out from worse information than it
    /// should have been.
    Degraded,
}

/// A reason this review might be wrong. Dagger's own findings and adapter notes share
/// one list; `impact` is what a reader acts on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct Warning {
    pub impact: Impact,
    /// Worded here rather than on the page, so there's one wording and names can be
    /// looked up while they're still to hand.
    pub message: String,
    /// What it's about, when it's about one definition. For pointing somebody at it.
    pub about: Option<Identity>,
}

/// One definition, and everything this review knows about it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct Definition {
    /// Whether it holds others. Said by whoever read the file.
    pub role: Role,
    /// What it was on each side.
    pub sides: Sides,
    /// What happened to it.
    pub change: Change,
    /// Hops from the nearest change that reached it. Never zero: its own change is `change`.
    pub reached: Option<NonZeroU32>,
    /// The one word for what happened to it, as every rendering shows it.
    pub mark: Mark,
    /// What it's written inside. Always present in this review when it isn't `None`.
    pub parent: Option<Identity>,
}

/// Everything one review of a repository comes to.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct Review {
    /// The commit's title, when the adapter knows one. A directory has no commit message.
    pub title: Option<String>,
    pub definitions: BTreeMap<Identity, Definition>,
    /// What to read, in order. Whatever a step names is worth reading; everything else in
    /// `definitions` is here to be drawn around it.
    pub reading: Vec<Step>,
    pub edges: Vec<Edge>,
    /// Everything drawn, as it nests and stacks, so the reading and the page can't disagree.
    pub groups: Vec<Group>,
    /// What a reader calls the grouping: "package", "crate".
    pub grouping: Option<String>,
    /// How far this review was told to follow a change outward.
    pub ripples: u32,
    pub cost: Cost,
    pub warnings: Vec<Warning>,
}

/// `notes` are the adapters' warnings, already worded. `found` is what dagger noticed,
/// worded here where the definitions are still to hand.
pub fn review(
    definitions: Vec<model::Definition>,
    references: &[Reference],
    ripples: u32,
    grouping: &Grouping,
    notes: Vec<Warning>,
    found: Vec<Diagnostic>,
    title: Option<String>,
) -> Review {
    let (classified, lopsided): (Vec<(&model::Definition, Change)>, Vec<Option<Diagnostic>>) =
        definitions
            .iter()
            .map(|def| {
                let (change, found) = classify(def);
                ((def, change), found)
            })
            .unzip();
    let changes: BTreeMap<Identity, Change> = classified
        .iter()
        .map(|(def, change)| (def.identity, change.clone()))
        .collect();

    let (reached, unbound) = reached(&changes, references, ripples);
    let findings: Vec<Diagnostic> = found
        .into_iter()
        .chain(lopsided.into_iter().flatten())
        .chain(unbound)
        .collect();

    let warnings: Vec<Warning> = notes
        .into_iter()
        .chain(findings.iter().map(|one| told(one, &definitions)))
        .collect();

    let read: BTreeSet<Identity> = changes
        .iter()
        .filter(|(_, change)| change.worth_reading())
        .map(|(identity, _)| *identity)
        .chain(reached.keys().copied())
        .collect();

    let latest: BTreeMap<Identity, &model::Occurrence> = definitions
        .iter()
        .map(|def| (def.identity, def.sides.latest()))
        .collect();
    let parent_of: BTreeMap<Identity, Identity> = {
        let by_name: BTreeMap<&Locator, Identity> = latest
            .iter()
            .map(|(identity, it)| (&it.locator, *identity))
            .collect();
        latest
            .iter()
            .filter_map(|(identity, it)| Some((*identity, *by_name.get(it.parent.as_ref()?)?)))
            .collect()
    };

    // Everything worth reading plus every container around it, so a box is never drawn
    // around something whose container isn't here.
    let shown: BTreeSet<Identity> = read
        .iter()
        .flat_map(|&one| std::iter::successors(Some(one), |at| parent_of.get(at).copied()))
        .collect();

    let edges = project(&shown, references);

    let kept: BTreeMap<Identity, Definition> = classified
        .into_iter()
        .filter(|(def, _)| shown.contains(&def.identity))
        .map(|(def, change)| {
            let identity = def.identity;
            (
                identity,
                Definition {
                    role: def.sides.latest().role,
                    mark: change.mark(reached.contains_key(&identity)),
                    change,
                    reached: reached.get(&identity).copied(),
                    parent: parent_of.get(&identity).copied(),
                    sides: def.sides.clone(),
                },
            )
        })
        .collect();

    let shape = Shape::new(&kept, grouping);
    let ordering = order(&read, &edges, &definitions, grouping);
    let groups = shape.groups(&edges, &ordering.steps);

    Review {
        title,
        groups,
        grouping: (!grouping.name.is_empty()).then(|| grouping.name.clone()),
        definitions: kept,
        reading: ordering.steps,
        cost: ordering.cost,
        edges,
        ripples,
        warnings,
    }
}

/// One of dagger's own findings, worded. Names are looked up here so five findings of
/// one kind can be told apart.
fn told(found: &Diagnostic, definitions: &[model::Definition]) -> Warning {
    let named = |identity: Identity| match definitions
        .iter()
        .find(|def| def.identity == identity)
        .map(|def| def.sides.latest())
    {
        Some(shows) => format!("{} ({})", shows.locator, shows.file),
        None => format!("definition {}", identity.0),
    };

    let (impact, message) = match found {
        Diagnostic::Unattributed { file, lines, at } => (
            Impact::Incomplete,
            format!(
                "{file}: {lines} changed line{} belong to no definition, around line {}",
                if *lines == 1 { "" } else { "s" },
                at.iter().map(u32::to_string).collect::<Vec<_>>().join(", ")
            ),
        ),
        Diagnostic::UnboundInType { definition, symbol } => (
            Impact::Incomplete,
            format!(
                "{symbol} appears where callers of {} can see it, but nothing could say \
                 what it refers to",
                named(*definition)
            ),
        ),
        Diagnostic::MentionFromNowhere { from } => (
            Impact::Incomplete,
            format!("a mention came from {from}, which was never reported"),
        ),
        Diagnostic::Tangled { definition, at } => (
            Impact::Incomplete,
            format!(
                "{definition} was handed over with two of its pieces covering the same text, \
                 around byte {at} — so a line of it is shown twice, and read as two different \
                 kinds of change"
            ),
        ),
        Diagnostic::TwoOfOneName { locator, times } => (
            Impact::Incomplete,
            format!(
                "{times} definitions are called {locator}, so only one of them could be \
                 followed from one side to the other"
            ),
        ),
        Diagnostic::LopsidedType { definition } => (
            Impact::Degraded,
            format!(
                "{}: the compiler described one side of this and not the other, so the \
                 signature as written was compared instead",
                named(*definition)
            ),
        ),
    };

    Warning {
        impact,
        message,
        about: found.about(),
    }
}

/// A mention counts if it was there in either snapshot, so that a deleted definition
/// still hangs off whatever it used to call.
fn dependencies(references: &[Reference]) -> BTreeMap<Identity, Vec<Identity>> {
    references
        .iter()
        .filter_map(|reference| match reference.to {
            Target::Known(to) if reference.from != to => Some((reference.from, to)),
            _ => None,
        })
        .fold(BTreeMap::new(), |mut out, (from, to)| {
            out.entry(from).or_default().push(to);
            out
        })
}

/// Walk out from each definition worth drawing, stepping over anything the reader won't
/// see, so two changed definitions joined by an untouched helper still look joined.
fn project(shown: &BTreeSet<Identity>, references: &[Reference]) -> Vec<Edge> {
    let dependencies = dependencies(references);
    let mut edges = Vec::new();

    for from in shown {
        let mut seen = BTreeSet::from([*from]);
        let mut queue: VecDeque<Identity> = dependencies
            .get(from)
            .into_iter()
            .flatten()
            .copied()
            .collect();

        while let Some(node) = queue.pop_front() {
            if !seen.insert(node) {
                continue;
            }
            if shown.contains(&node) {
                edges.push(Edge {
                    from: *from,
                    to: node,
                });
                continue;
            }
            queue.extend(dependencies.get(&node).into_iter().flatten().copied());
        }
    }

    edges
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Part, Sides};
    use crate::testing::{occurrence, reference};

    fn edited(id: u32, name: &str) -> model::Definition {
        model::Definition {
            identity: Identity(id),
            sides: Sides::Kept {
                before: occurrence(name, &[(Part::Body, "old")]),
                after: occurrence(name, &[(Part::Body, "new")]),
            },
        }
    }

    fn untouched(id: u32, name: &str) -> model::Definition {
        model::Definition {
            identity: Identity(id),
            sides: Sides::Kept {
                before: occurrence(name, &[(Part::Body, "same")]),
                after: occurrence(name, &[(Part::Body, "same")]),
            },
        }
    }

    fn reviewed(
        definitions: Vec<model::Definition>,
        references: &[Reference],
        ripples: u32,
    ) -> Review {
        review(
            definitions,
            references,
            ripples,
            &Grouping::default(),
            Vec::new(),
            Vec::new(),
            None,
        )
    }

    /// Everything handed over is either worth reading or holds something that is.
    #[test]
    fn only_definitions_worth_reading_get_in() {
        let review = reviewed(
            vec![edited(0, "lookupPrice"), untouched(1, "withRetry")],
            &[],
            1,
        );

        assert_eq!(
            review.definitions.keys().copied().collect::<Vec<_>>(),
            vec![Identity(0)]
        );
        assert_eq!(review.reading.len(), 1);
    }

    /// buildLineItems calls withRetry calls lookupPrice. The helper is untouched and
    /// generic, so the reader never sees it, but the two edits are still related.
    #[test]
    fn an_untouched_helper_is_stepped_over() {
        let review = reviewed(
            vec![
                edited(0, "buildLineItems"),
                untouched(1, "withRetry"),
                edited(2, "lookupPrice"),
            ],
            &[reference(0, 1, Part::Body), reference(1, 2, Part::Body)],
            u32::MAX,
        );

        assert_eq!(
            review.edges,
            vec![Edge {
                from: Identity(0),
                to: Identity(2),
            }]
        );
    }

    #[test]
    fn a_direct_mention_is_an_edge() {
        let review = reviewed(
            vec![edited(0, "cartTotal"), edited(1, "lineTotal")],
            &[reference(0, 1, Part::Body)],
            u32::MAX,
        );

        assert_eq!(
            review.edges,
            vec![Edge {
                from: Identity(0),
                to: Identity(1),
            }]
        );
    }

    #[test]
    fn a_dead_end_helper_produces_no_edge() {
        let review = reviewed(
            vec![edited(0, "cartTotal"), untouched(1, "log")],
            &[reference(0, 1, Part::Body)],
            u32::MAX,
        );

        assert!(review.edges.is_empty());
    }

    /// Every identity named anywhere is a definition that was handed over.
    #[test]
    fn nothing_names_a_definition_that_was_not_handed_over() {
        let review = reviewed(
            vec![
                edited(0, "buildLineItems"),
                untouched(1, "withRetry"),
                edited(2, "lookupPrice"),
            ],
            &[reference(0, 1, Part::Body), reference(1, 2, Part::Body)],
            u32::MAX,
        );

        let known = |identity: &Identity| review.definitions.contains_key(identity);
        for edge in &review.edges {
            assert!(
                known(&edge.from) && known(&edge.to),
                "edge {edge:?} dangles"
            );
        }
        for step in &review.reading {
            assert!(known(&step.definition), "a step names nobody");
            assert!(step.on_faith.iter().all(known), "on_faith names nobody");
        }
        for (identity, one) in &review.definitions {
            if let Some(parent) = one.parent {
                assert!(known(&parent), "{identity:?} is inside nobody");
                assert_eq!(
                    review.definitions[&parent].role,
                    Role::Container,
                    "{identity:?} is inside something that holds nothing"
                );
            }
        }
        for warning in &review.warnings {
            assert!(warning.about.is_none_or(|about| known(&about)));
            assert!(!warning.message.is_empty(), "a warning nobody can read");
        }
    }

    /// Nothing is written inside itself, however the extractors describe it.
    #[test]
    fn containment_does_not_go_in_circles() {
        let review = reviewed(vec![edited(0, "one"), edited(1, "two")], &[], 1);

        for identity in review.definitions.keys() {
            let mut seen = BTreeSet::from([*identity]);
            let mut at = review.definitions[identity].parent;
            while let Some(one) = at {
                assert!(seen.insert(one), "{identity:?} is inside itself");
                at = review.definitions[&one].parent;
            }
        }
    }
}
