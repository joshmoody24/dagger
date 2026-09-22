//! What holds what, and how it stacks.
//!
//! A group from the grouping holds definitions, a definition holds whatever is written
//! inside it, and at every level the same question has the same answer: whatever holds
//! something up sits above it, and along one tier whichever is read first comes first.
//! Worked out here, once, so the reading and the page can't disagree about it. They used
//! to: the page worked the tiers out again for itself from the edges, and every place the
//! two workings differed was a bug that looked like a layout bug.

use crate::group::Grouping;
use crate::model::Identity;
use crate::order::Step;
use crate::review::{Definition, Edge};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// One box on the page, or one thing in it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Group {
    /// A box: a group from the grouping, or a definition with others written inside it. A
    /// definition's box holds its own node when there's something in it to read, which is
    /// what a reader reads and points at.
    Group {
        name: String,
        /// How far down its holder it sits: nought for whatever leans on nothing else
        /// there, one more than the furthest thing it leans on otherwise.
        tier: u32,
        /// Tiers ascending, and along a tier in reading order.
        children: Vec<Group>,
    },
    Node {
        id: Identity,
        tier: u32,
    },
}

impl Group {
    pub fn tier(&self) -> u32 {
        match self {
            Group::Group { tier, .. } | Group::Node { tier, .. } => *tier,
        }
    }

    /// Every definition in it, however deep.
    fn within(&self, out: &mut BTreeSet<Identity>) {
        match self {
            Group::Node { id, .. } => {
                out.insert(*id);
            }
            Group::Group { children, .. } => {
                children.iter().for_each(|one| one.within(out));
            }
        }
    }
}

/// What's written inside what, and which group the outermost things are in.
pub struct Shape<'a> {
    definitions: &'a BTreeMap<Identity, Definition>,
    children: BTreeMap<Identity, Vec<Identity>>,
    /// The outermost definitions of each group. An empty path is no group at all: those
    /// sit at the top beside the groups, each on its own.
    outermost: BTreeMap<Vec<String>, Vec<Identity>>,
}

impl<'a> Shape<'a> {
    pub fn new(definitions: &'a BTreeMap<Identity, Definition>, grouping: &Grouping) -> Self {
        let mut children: BTreeMap<Identity, Vec<Identity>> = BTreeMap::new();
        let mut outermost: BTreeMap<Vec<String>, Vec<Identity>> = BTreeMap::new();
        for (id, one) in definitions {
            match one.parent {
                Some(up) => children.entry(up).or_default().push(*id),
                None => outermost
                    .entry(grouping.path_of(*id).to_vec())
                    .or_default()
                    .push(*id),
            }
        }
        Shape {
            definitions,
            children,
            outermost,
        }
    }

    /// The tier of the top-level box or node each definition is in. What the reading goes
    /// by, so that it runs down the page.
    pub fn tops(&self, edges: &[Edge]) -> BTreeMap<Identity, u32> {
        let units: Vec<BTreeSet<Identity>> = self
            .outermost
            .iter()
            .flat_map(|(path, held)| {
                let units = held.iter().map(|id| self.all_within(*id));
                if path.is_empty() {
                    units.collect()
                } else {
                    vec![units.flatten().collect()]
                }
            })
            .collect();
        let tiers = tiers(&units, edges);
        units
            .into_iter()
            .zip(tiers)
            .flat_map(|(members, tier)| members.into_iter().map(move |id| (id, tier)))
            .collect()
    }

    /// Everything drawn, as it nests.
    pub fn groups(&self, edges: &[Edge], steps: &[Step]) -> Vec<Group> {
        let first: BTreeMap<Identity, usize> = steps
            .iter()
            .enumerate()
            .map(|(at, step)| (step.definition, at))
            .collect();
        let top = self
            .outermost
            .iter()
            .flat_map(|(path, held)| {
                let built = held.iter().map(|id| self.build(*id, edges, &first));
                if path.is_empty() {
                    built.collect()
                } else {
                    vec![Group::Group {
                        name: path.join("/"),
                        tier: 0,
                        children: arranged(built.collect(), edges, &first),
                    }]
                }
            })
            .collect();
        arranged(top, edges, &first)
    }

    /* A node is something to read. A definition here only to be drawn around what's read
     * — a module nothing changed in — is its box and no more. */
    fn build(&self, id: Identity, edges: &[Edge], first: &BTreeMap<Identity, usize>) -> Group {
        let node = Group::Node { id, tier: 0 };
        match self.children.get(&id) {
            None => node,
            Some(held) => {
                let inside = first
                    .contains_key(&id)
                    .then_some(node)
                    .into_iter()
                    .chain(held.iter().map(|child| self.build(*child, edges, first)))
                    .collect();
                Group::Group {
                    name: self.definitions[&id].sides.latest().locator.name.clone(),
                    tier: 0,
                    children: arranged(inside, edges, first),
                }
            }
        }
    }

    fn all_within(&self, id: Identity) -> BTreeSet<Identity> {
        let mut out = BTreeSet::from([id]);
        let mut queue = vec![id];
        while let Some(one) = queue.pop() {
            for child in self.children.get(&one).into_iter().flatten() {
                if out.insert(*child) {
                    queue.push(*child);
                }
            }
        }
        out
    }
}

/// The same things with their tiers filled in, tiers ascending and along a tier in reading
/// order. A box counts as one thing: it leans on whatever its contents lean on outside it.
fn arranged(mut held: Vec<Group>, edges: &[Edge], first: &BTreeMap<Identity, usize>) -> Vec<Group> {
    let units: Vec<BTreeSet<Identity>> = held
        .iter()
        .map(|one| {
            let mut out = BTreeSet::new();
            one.within(&mut out);
            out
        })
        .collect();
    let soonest: Vec<usize> = units
        .iter()
        .map(|members| {
            members
                .iter()
                .filter_map(|id| first.get(id))
                .min()
                .copied()
                .unwrap_or(usize::MAX)
        })
        .collect();
    for (one, tier) in held.iter_mut().zip(tiers(&units, edges)) {
        match one {
            Group::Group { tier: own, .. } | Group::Node { tier: own, .. } => *own = tier,
        }
    }
    let mut order: Vec<usize> = (0..held.len()).collect();
    order.sort_by_key(|&at| (held[at].tier(), soonest[at]));
    let mut taken: Vec<Option<Group>> = held.into_iter().map(Some).collect();
    order
        .into_iter()
        .map(|at| taken[at].take().expect("each index once"))
        .collect()
}

/// Nought for a unit that leans on nothing else here, one more than the furthest it leans
/// on otherwise. A circle is settled by whichever unit is asked first, which is enough:
/// being in one means there is no right answer, only a readable one.
fn tiers(units: &[BTreeSet<Identity>], edges: &[Edge]) -> Vec<u32> {
    let unit_of: BTreeMap<Identity, usize> = units
        .iter()
        .enumerate()
        .flat_map(|(at, members)| members.iter().map(move |id| (*id, at)))
        .collect();
    let mut leans: Vec<BTreeSet<usize>> = vec![BTreeSet::new(); units.len()];
    for edge in edges {
        if let (Some(&from), Some(&to)) = (unit_of.get(&edge.from), unit_of.get(&edge.to))
            && from != to
        {
            leans[from].insert(to);
        }
    }

    fn depth(at: usize, leans: &[BTreeSet<usize>], seen: &mut [Option<u32>]) -> u32 {
        if let Some(found) = seen[at] {
            return found;
        }
        seen[at] = Some(0);
        let found = leans[at]
            .iter()
            .map(|&other| depth(other, leans, seen) + 1)
            .max()
            .unwrap_or(0);
        seen[at] = Some(found);
        found
    }
    let mut seen = vec![None; units.len()];
    (0..units.len())
        .map(|at| depth(at, &leans, &mut seen))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::change::Change;
    use crate::model::{Part, Role, Sides};
    use crate::testing::occurrence;

    /// A definition written inside `parent`.
    fn one(id: u32, name: &str, parent: Option<u32>) -> (Identity, Definition) {
        (
            Identity(id),
            Definition {
                role: Role::Item,
                sides: Sides::Added(occurrence(name, &[(Part::Body, "new")])),
                change: Change::Added,
                reached: None,
                parent: parent.map(Identity),
            },
        )
    }

    fn edge(from: u32, to: u32) -> Edge {
        Edge {
            from: Identity(from),
            to: Identity(to),
        }
    }

    fn grouping(of: &[(u32, &[&str])]) -> Grouping {
        Grouping {
            name: String::new(),
            of: of
                .iter()
                .map(|(id, path)| (Identity(*id), path.iter().map(|s| s.to_string()).collect()))
                .collect(),
        }
    }

    fn shaped(
        definitions: &[(Identity, Definition)],
        edges: &[Edge],
        reading: &[u32],
    ) -> Vec<Group> {
        let definitions: BTreeMap<Identity, Definition> = definitions.iter().cloned().collect();
        let steps: Vec<Step> = reading
            .iter()
            .map(|id| Step {
                definition: Identity(*id),
                on_faith: Vec::new(),
            })
            .collect();
        Shape::new(&definitions, &grouping(&[])).groups(edges, &steps)
    }

    /// The node for `id`, wherever it sits.
    fn node(groups: &[Group], id: u32) -> Option<&Group> {
        groups.iter().find_map(|one| match one {
            Group::Node { id: own, .. } => (own.0 == id).then_some(one),
            Group::Group { children, .. } => node(children, id),
        })
    }

    /// The box called `name`, wherever it sits.
    fn boxed<'g>(groups: &'g [Group], name: &str) -> Option<&'g Group> {
        groups.iter().find_map(|one| match one {
            Group::Group { name: own, .. } if own == name => Some(one),
            Group::Group { children, .. } => boxed(children, name),
            Group::Node { .. } => None,
        })
    }

    /// What each child is: a node's id, or a box's name.
    fn along(children: &[Group]) -> Vec<String> {
        children
            .iter()
            .map(|child| match child {
                Group::Node { id, .. } => id.0.to_string(),
                Group::Group { name, .. } => name.clone(),
            })
            .collect()
    }

    /* A test leans on a helper, so the helper holds it up, so the helper goes above — with
     * the box of tests counting as one thing that leans on whatever its contents do. */
    #[test]
    fn what_leans_on_the_code_around_it_sits_below_it_box_or_not() {
        let groups = shaped(
            &[
                one(1, "lib", None),
                one(2, "helper", Some(1)),
                one(3, "tests", Some(1)),
                one(4, "checks_helper", Some(3)),
            ],
            &[edge(4, 2)],
            &[1, 2, 4],
        );
        assert_eq!(node(&groups, 2).unwrap().tier(), 0);
        assert_eq!(
            boxed(&groups, "tests").unwrap().tier(),
            1,
            "the box of tests, below the helper"
        );
    }

    /* A definition with others inside it is a box holding its own node, so a reader has
     * something to read and point at; one with nothing inside is just a node. */
    #[test]
    fn a_box_holds_its_own_node_and_an_empty_one_is_a_node() {
        let groups = shaped(
            &[one(1, "lib", None), one(2, "Alias", Some(1))],
            &[],
            &[1, 2],
        );
        let lib = boxed(&groups, "lib").unwrap();
        assert!(matches!(lib, Group::Group { children, .. } if along(children) == ["1", "2"]));
        assert!(boxed(&groups, "Alias").is_none());
    }

    /* A box is a definition too: something leaning on it holds it up like anything else. */
    #[test]
    fn a_box_that_something_leans_on_is_a_dependency_like_any_other() {
        let groups = shaped(
            &[
                one(1, "field", None),
                one(2, "Unit", None),
                one(3, "value", Some(2)),
            ],
            &[edge(1, 2)],
            &[2, 3, 1],
        );
        assert_eq!(boxed(&groups, "Unit").unwrap().tier(), 0);
        assert_eq!(node(&groups, 1).unwrap().tier(), 1);
    }

    /* At one tier the order along it is the reading's to say, a box no more than a node. */
    #[test]
    fn along_a_tier_the_reading_decides() {
        let definitions = [
            one(1, "lib", None),
            one(2, "helper", Some(1)),
            one(3, "used", Some(1)),
            one(4, "tests", Some(1)),
            one(5, "checks", Some(4)),
        ];
        let edges = [edge(3, 2), edge(5, 2)];
        let inside = |reading: &[u32]| match shaped(&definitions, &edges, reading).as_slice() {
            [Group::Group { children, .. }] => along(children),
            other => panic!("one box, not {other:?}"),
        };
        assert_eq!(inside(&[1, 2, 3, 5]), ["1", "2", "3", "tests"]);
        assert_eq!(inside(&[1, 2, 5, 3]), ["1", "2", "tests", "3"]);
    }

    /* Groups stack the same way, and whatever is in no group sits beside them. */
    #[test]
    fn a_group_sits_below_what_it_leans_on_and_the_ungrouped_beside_them() {
        let definitions: BTreeMap<Identity, Definition> = [
            one(1, "main", None),
            one(2, "util", None),
            one(3, "loose", None),
        ]
        .into_iter()
        .collect();
        let grouping = grouping(&[(1, &["app"]), (2, &["lib"])]);
        let tops = Shape::new(&definitions, &grouping).tops(&[edge(1, 2), edge(3, 1)]);
        assert_eq!(tops[&Identity(2)], 0);
        assert_eq!(tops[&Identity(1)], 1);
        assert_eq!(tops[&Identity(3)], 2);
    }
}
