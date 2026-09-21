//! The order to read a change in.
//!
//! Two rules, and the second one is the whole idea. Don't read something before the
//! things it leans on. And keep as little in the reader's head at once as possible.
//!
//! Something is *open* once it's been read and something still unread depends on it:
//! it has to be kept in mind. It closes when the last thing depending on it is read,
//! which is the satisfying part of finishing a branch. Something taken *on faith* is
//! the opposite, a promise held about code not yet seen, which only happens when
//! definitions depend on each other in a circle and there's no honest place to start.
//!
//! Candidates are compared in order — faith first, then how much is left open, then whether
//! it drags the reader somewhere else — rather than scored and added up, so there are no
//! weights to argue about.

use crate::group::Grouping;
use crate::model::{Definition, Identity};
use crate::review::Review;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct Step {
    pub definition: Identity,
    /// What this leans on that hasn't been read yet. Only ever non-empty inside a
    /// circle, and kept as short as we can manage.
    pub on_faith: Vec<Identity>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct Ordering {
    pub steps: Vec<Step>,
    /// How this reading went, so one rule can be argued against another with numbers
    /// from real changes rather than taste.
    pub cost: Cost,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct Cost {
    /// The most that was in the reader's head at once.
    pub peak_open: usize,
    /// Added up over every step, so a long shallow reading can be told apart from a
    /// short deep one.
    pub total_open: usize,
    pub taken_on_faith: usize,
    /// Steps that landed somewhere other than the one before: another package, or failing
    /// a grouping, another file.
    pub jumps: usize,
}

pub fn order(review: &Review, definitions: &[Definition], grouping: &Grouping) -> Ordering {
    let members: Vec<Identity> = review.members.iter().copied().collect();
    let places: BTreeMap<Identity, usize> = members
        .iter()
        .enumerate()
        .map(|(place, identity)| (*identity, place))
        .collect();

    // Where each one sits, for judging whether reading the next takes the reader somewhere
    // else. A grouping says where if it has an opinion; failing that, the file it's in.
    let wheres: Vec<&[String]> = {
        let files: BTreeMap<Identity, &String> = definitions
            .iter()
            .map(|definition| (definition.identity, &definition.sides.latest().file))
            .collect();
        members
            .iter()
            .map(|identity| {
                let grouped = grouping.path_of(*identity);
                if grouped.is_empty() {
                    files
                        .get(identity)
                        .map(|file| std::slice::from_ref(*file))
                        .unwrap_or_default()
                } else {
                    grouped
                }
            })
            .collect()
    };

    let (leans_on, holds_up) = relations(review, &places, members.len());

    let mut read = vec![false; members.len()];
    let mut unread_leans: Vec<usize> = leans_on.iter().map(BTreeSet::len).collect();
    let mut unread_holds: Vec<usize> = holds_up.iter().map(BTreeSet::len).collect();

    let mut steps = Vec::with_capacity(members.len());
    let mut cost = Cost::default();
    let mut open = 0usize;
    let mut last_where: Option<&[String]> = None;

    for _ in 0..members.len() {
        let Some(next) = pick(
            &read,
            &unread_leans,
            &unread_holds,
            &leans_on,
            &wheres,
            last_where,
        ) else {
            break;
        };

        let on_faith: Vec<Identity> = leans_on[next]
            .iter()
            .filter(|&&leaned| !read[leaned])
            .map(|&leaned| members[leaned])
            .collect();

        read[next] = true;
        if unread_holds[next] > 0 {
            open += 1;
        }
        for &leaned in &leans_on[next] {
            unread_holds[leaned] -= 1;
            if read[leaned] && unread_holds[leaned] == 0 {
                open -= 1;
            }
        }
        for &holder in &holds_up[next] {
            unread_leans[holder] -= 1;
        }

        if last_where.is_some_and(|was| was != wheres[next]) {
            cost.jumps += 1;
        }
        last_where = Some(wheres[next]);
        cost.peak_open = cost.peak_open.max(open);
        cost.total_open += open;
        cost.taken_on_faith += on_faith.len();

        steps.push(Step {
            definition: members[next],
            on_faith,
        });
    }

    Ordering { steps, cost }
}

/// Which members lean on which, and the same the other way round.
fn relations(
    review: &Review,
    places: &BTreeMap<Identity, usize>,
    count: usize,
) -> (Vec<BTreeSet<usize>>, Vec<BTreeSet<usize>>) {
    let mut leans_on = vec![BTreeSet::new(); count];
    let mut holds_up = vec![BTreeSet::new(); count];

    for edge in &review.edges {
        if let (Some(&from), Some(&to)) = (places.get(&edge.from), places.get(&edge.to))
            && from != to
        {
            leans_on[from].insert(to);
            holds_up[to].insert(from);
        }
    }

    (leans_on, holds_up)
}

/// The next one to read, judged on each count in turn: how much has to be taken on faith,
/// then how much is left in the reader's head, then whether it means moving somewhere else.
/// Position settles the rest so the same change always reads the same.
fn pick(
    read: &[bool],
    unread_leans: &[usize],
    unread_holds: &[usize],
    leans_on: &[BTreeSet<usize>],
    wheres: &[&[String]],
    last_where: Option<&[String]>,
) -> Option<usize> {
    (0..read.len())
        .filter(|&candidate| !read[candidate])
        .min_by_key(|&candidate| {
            let closes = leans_on[candidate]
                .iter()
                .filter(|&&leaned| read[leaned] && unread_holds[leaned] == 1)
                .count();
            let opens = usize::from(unread_holds[candidate] > 0);

            (
                unread_leans[candidate],
                opens as isize - closes as isize,
                usize::from(last_where.is_some_and(|was| was != wheres[candidate])),
                candidate,
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::change::{Change, Edits};
    use crate::model::{Part, Sides};
    use crate::review::Edge;
    use crate::testing::occurrence;

    /// A review of `count` definitions, all edited, wired up by the given pairs. Each
    /// pair reads "the first leans on the second".
    fn built(count: u32, leans: &[(u32, u32)], files: &[&str]) -> (Review, Vec<Definition>) {
        let definitions: Vec<Definition> = (0..count)
            .map(|id| {
                let mut before = occurrence(&format!("d{id}"), &[(Part::Body, "old")]);
                let mut after = occurrence(&format!("d{id}"), &[(Part::Body, "new")]);
                let file = files.get(id as usize).copied().unwrap_or("one.rs");
                before.file = file.to_string();
                after.file = file.to_string();
                Definition {
                    identity: Identity(id),
                    sides: Sides::Kept { before, after },
                }
            })
            .collect();

        let review = Review {
            changes: (0..count)
                .map(|id| {
                    (
                        Identity(id),
                        Change::Kept(Edits {
                            parts: BTreeSet::from([Part::Body]),
                            ..Edits::default()
                        }),
                    )
                })
                .collect(),
            affected: BTreeSet::new(),
            members: (0..count).map(Identity).collect(),
            context: BTreeSet::new(),
            edges: leans
                .iter()
                .map(|(from, to)| Edge {
                    from: Identity(*from),
                    to: Identity(*to),
                    via: Vec::new(),
                })
                .collect(),
            diagnostics: Vec::new(),
        };

        (review, definitions)
    }

    fn reading(count: u32, leans: &[(u32, u32)]) -> Vec<u32> {
        let (review, definitions) = built(count, leans, &[]);
        order(&review, &definitions, &Grouping::default())
            .steps
            .iter()
            .map(|step| step.definition.0)
            .collect()
    }

    #[test]
    fn nothing_is_read_before_what_it_leans_on() {
        // 0 leans on 1 leans on 2.
        assert_eq!(reading(3, &[(0, 1), (1, 2)]), vec![2, 1, 0]);
    }

    #[test]
    fn everything_gets_read_exactly_once() {
        let reading = reading(6, &[(0, 1), (1, 2), (3, 4), (5, 0)]);

        assert_eq!(reading.len(), 6);
        assert_eq!(reading.iter().copied().collect::<BTreeSet<u32>>().len(), 6);
    }

    /// 0 and 1 both lean on 2. Reading 2 first leaves two things open at once, but
    /// there's no way around it, and the pair that closes it should follow straight on.
    #[test]
    fn a_shared_foundation_comes_first() {
        let reading = reading(3, &[(0, 2), (1, 2)]);

        assert_eq!(reading[0], 2);
    }

    /// Two separate chains. Finishing one before starting the other keeps fewer things
    /// in mind than alternating between them.
    #[test]
    fn one_chain_is_finished_before_the_next_is_started() {
        let reading = reading(4, &[(0, 1), (2, 3)]);
        let chains: Vec<bool> = reading.iter().map(|id| *id < 2).collect();

        assert!(
            chains == vec![true, true, false, false] || chains == vec![false, false, true, true],
            "chains were interleaved: {reading:?}"
        );
    }

    /// Reading something nothing depends on costs nothing, while reading a foundation
    /// leaves a door open, so the free-standing bits come first. It's why a change to
    /// a lockfile or a build script lands at the top, before any code.
    #[test]
    fn what_nothing_leans_on_is_read_before_what_holds_things_up() {
        // 0 leans on 2, and 1 stands alone.
        assert_eq!(reading(3, &[(0, 2)]), vec![1, 2, 0]);
    }

    #[test]
    fn a_circle_is_read_with_one_thing_taken_on_faith() {
        let (review, definitions) = built(3, &[(0, 1), (1, 2), (2, 0)], &[]);
        let ordering = order(&review, &definitions, &Grouping::default());

        assert_eq!(ordering.steps.len(), 3);
        assert_eq!(ordering.cost.taken_on_faith, 1);
    }

    #[test]
    fn nothing_is_taken_on_faith_when_it_doesnt_have_to_be() {
        let (review, definitions) = built(4, &[(0, 1), (1, 2), (2, 3)], &[]);

        assert_eq!(
            order(&review, &definitions, &Grouping::default())
                .cost
                .taken_on_faith,
            0
        );
    }

    /// Same graph, but 1 sits in another file. Since either order is otherwise equal,
    /// the reading should stay put rather than hop out and back.
    #[test]
    fn a_reading_would_rather_stay_in_one_file() {
        let (review, definitions) = built(3, &[(0, 1), (0, 2)], &["a.rs", "b.rs", "a.rs"]);
        let ordering = order(&review, &definitions, &Grouping::default());

        assert_eq!(ordering.cost.jumps, 1);
    }

    #[test]
    fn a_chain_keeps_only_one_thing_in_mind_at_a_time() {
        let (review, definitions) = built(5, &[(0, 1), (1, 2), (2, 3), (3, 4)], &[]);

        assert_eq!(
            order(&review, &definitions, &Grouping::default())
                .cost
                .peak_open,
            1
        );
    }

    #[test]
    fn unrelated_definitions_leave_nothing_open() {
        let (review, definitions) = built(4, &[], &[]);

        assert_eq!(
            order(&review, &definitions, &Grouping::default())
                .cost
                .peak_open,
            0
        );
    }
}
