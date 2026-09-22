//! The order to read a change in: nothing before what it depends on, then finish the
//! current branch before moving on, then foundations before what stands on them.
//!
//! A definition is *open* once read while something unread still depends on it. One is
//! taken *on faith* when read before a dependency, which only happens in a cycle.
//! Candidates are compared criterion by criterion rather than scored, so there are no
//! weights to tune; `pick` lists the criteria.

use crate::group::Grouping;
use crate::model::{self, Identity, Locator, Role};
use crate::review::Edge;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct Step {
    pub definition: Identity,
    /// Dependencies not yet read. Only non-empty inside a cycle.
    pub on_faith: Vec<Identity>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct Ordering {
    pub steps: Vec<Step>,
    /// Numbers for comparing one ordering rule against another on real changes.
    pub cost: Cost,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct Cost {
    /// The most open at once.
    pub peak_open: usize,
    /// Summed over every step, to tell a long shallow reading from a short deep one.
    pub total_open: usize,
    pub taken_on_faith: usize,
    /// Steps that moved to another package, or without a grouping, another file.
    pub jumps: usize,
}

/// `read` is what's worth reading; `edges` is what depends on what among everything drawn.
pub fn order(
    read: &BTreeSet<Identity>,
    edges: &[Edge],
    definitions: &[model::Definition],
    grouping: &Grouping,
) -> Ordering {
    let members: Vec<Identity> = read.iter().copied().collect();
    let places: BTreeMap<Identity, usize> = members
        .iter()
        .enumerate()
        .map(|(place, identity)| (*identity, place))
        .collect();

    let files: BTreeMap<Identity, &String> = definitions
        .iter()
        .map(|definition| (definition.identity, &definition.sides.latest().file))
        .collect();

    let enclosing = enclosing(definitions);
    let homes = homes(&members, definitions, &enclosing);
    // A container's header (a module's prose and imports, an impl's header) is read on
    // arriving at it, right before its contents.
    let headers: Vec<bool> = {
        let roles: BTreeMap<Identity, Role> = definitions
            .iter()
            .map(|definition| (definition.identity, definition.sides.latest().role))
            .collect();
        members
            .iter()
            .map(|identity| roles.get(identity) == Some(&Role::Container))
            .collect()
    };

    // Where each member sits, for counting jumps: the grouping's path if any, else the file.
    let wheres: Vec<&[String]> = {
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

    let (leans_on, holds_up) = relations(edges, &places, members.len(), &enclosing);

    let mut read = vec![false; members.len()];
    let mut unread_leans: Vec<usize> = leans_on.iter().map(BTreeSet::len).collect();
    let mut unread_holds: Vec<usize> = holds_up.iter().map(BTreeSet::len).collect();

    let mut steps = Vec::with_capacity(members.len());
    let mut cost = Cost::default();
    let mut open = 0usize;
    let mut last_where: Option<&[String]> = None;
    let mut last_home: Option<&str> = None;

    for _ in 0..members.len() {
        let Some(next) = pick(
            &read,
            &unread_leans,
            &unread_holds,
            &leans_on,
            &headers,
            Where {
                groups: &wheres,
                homes: &homes,
                group: last_where,
                home: last_home,
            },
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
        last_home = Some(homes[next].as_str());
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

/// Which members depend on which, and the reverse.
///
/// A container depends on whatever its contents depend on outside it. No edge ever says
/// so, and without it a module header would be read long before anything it holds.
fn relations(
    edges: &[Edge],
    places: &BTreeMap<Identity, usize>,
    count: usize,
    enclosing: &BTreeMap<Identity, Vec<Identity>>,
) -> (Vec<BTreeSet<usize>>, Vec<BTreeSet<usize>>) {
    let mut leans_on = vec![BTreeSet::new(); count];
    let mut holds_up = vec![BTreeSet::new(); count];
    let mut lean = |from: Identity, to: Identity| {
        if let (Some(&from), Some(&to)) = (places.get(&from), places.get(&to))
            && from != to
        {
            leans_on[from].insert(to);
            holds_up[to].insert(from);
        }
    };
    let around = |identity: &Identity| {
        enclosing
            .get(identity)
            .map(Vec::as_slice)
            .unwrap_or_default()
    };

    for edge in edges {
        lean(edge.from, edge.to);
        // Stop at the first container that also holds the target: nothing depends on
        // what's inside it.
        for &container in around(&edge.from) {
            if container == edge.to || around(&edge.to).contains(&container) {
                break;
            }
            lean(container, edge.to);
        }
    }

    (leans_on, holds_up)
}

/// What each definition is written inside, nearest first, within the same file. Parents
/// are given as names, so they're resolved by name within the file.
fn enclosing(definitions: &[model::Definition]) -> BTreeMap<Identity, Vec<Identity>> {
    let shown: BTreeMap<Identity, (&str, &Locator, Option<&Locator>)> = definitions
        .iter()
        .map(|definition| {
            let it = definition.sides.latest();
            (
                definition.identity,
                (it.file.as_str(), &it.locator, it.parent.as_ref()),
            )
        })
        .collect();
    let by_name: BTreeMap<(&str, &Locator), Identity> = shown
        .iter()
        .map(|(identity, (file, locator, _))| ((*file, *locator), *identity))
        .collect();

    shown
        .iter()
        .map(|(identity, (file, _, parent))| {
            let mut above = Vec::new();
            let mut held = *parent;
            while let Some(&up) = held.and_then(|name| by_name.get(&(*file, name))) {
                above.push(up);
                held = shown.get(&up).and_then(|(_, _, parent)| *parent);
            }
            (*identity, above)
        })
        .collect()
}

/// The module each member belongs to. Not the file, since a file can hold two modules.
/// The outermost container in a file is its module, whatever the language calls it; the
/// nearest container would wrongly make a method and a plain function two places.
fn homes(
    members: &[Identity],
    definitions: &[model::Definition],
    enclosing: &BTreeMap<Identity, Vec<Identity>>,
) -> Vec<String> {
    let shown: BTreeMap<Identity, (&str, &Locator)> = definitions
        .iter()
        .map(|definition| {
            let it = definition.sides.latest();
            (definition.identity, (it.file.as_str(), &it.locator))
        })
        .collect();

    // Neither a file name nor a path can contain a null, so the two halves can't be confused.
    let named = |file: &str, locator: &Locator| {
        let mut path = locator.scope.clone();
        path.push(locator.name.clone());
        format!("{file}\0{}", path.join("::"))
    };

    members
        .iter()
        .map(|identity| {
            let outermost = enclosing
                .get(identity)
                .and_then(|above| above.last())
                .unwrap_or(identity);
            match shown.get(outermost) {
                Some((file, locator)) => named(file, locator),
                None => String::new(),
            }
        })
        .collect()
}

/// Where the reader currently is, and where every member sits.
struct Where<'a> {
    groups: &'a [&'a [String]],
    homes: &'a [String],
    group: Option<&'a [String]>,
    home: Option<&'a str>,
}

impl Where<'_> {
    /// How far reading this one moves the reader: nowhere, out of the module, or out of
    /// the package. Module counts too, so a reading doesn't wander between modules freely.
    fn away(&self, candidate: usize) -> isize {
        match (self.group, self.home) {
            (Some(group), Some(home)) => {
                isize::from(home != self.homes[candidate].as_str())
                    + isize::from(group != self.groups[candidate])
            }
            _ => 0,
        }
    }
}

/// The next one to read, judged on each count in turn.
fn pick(
    read: &[bool],
    unread_leans: &[usize],
    unread_holds: &[usize],
    leans_on: &[BTreeSet<usize>],
    headers: &[bool],
    at: Where,
) -> Option<usize> {
    (0..read.len())
        .filter(|&candidate| !read[candidate])
        .min_by_key(|&candidate| {
            let closes = leans_on[candidate]
                .iter()
                .filter(|&&leaned| read[leaned] && unread_holds[leaned] == 1)
                .count();
            (
                // Never before what it depends on.
                unread_leans[candidate],
                // Stay in the current module; leaving means coming back to find the thread.
                at.away(candidate),
                // On arriving somewhere, read the container's header before its contents.
                isize::from(!headers[candidate]),
                // Then finish a branch, when one can be finished.
                -(closes as isize),
                // Then whatever the most things are waiting on.
                -(unread_holds[candidate] as isize),
                // Position settles the rest, so the same change always reads the same.
                candidate as isize,
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Part, Sides};
    use crate::review::Edge;
    use crate::testing::occurrence;

    /// `count` edited definitions, wired up by pairs of "the first depends on the second".
    fn built(
        count: u32,
        leans: &[(u32, u32)],
        files: &[&str],
    ) -> (BTreeSet<Identity>, Vec<Edge>, Vec<model::Definition>) {
        let definitions: Vec<model::Definition> = (0..count)
            .map(|id| {
                let mut before = occurrence(&format!("d{id}"), &[(Part::Body, "old")]);
                let mut after = occurrence(&format!("d{id}"), &[(Part::Body, "new")]);
                let file = files.get(id as usize).copied().unwrap_or("one.rs");
                before.file = file.to_string();
                after.file = file.to_string();
                model::Definition {
                    identity: Identity(id),
                    sides: Sides::Kept { before, after },
                }
            })
            .collect();

        let edges = leans
            .iter()
            .map(|(from, to)| Edge {
                from: Identity(*from),
                to: Identity(*to),
            })
            .collect();

        ((0..count).map(Identity).collect(), edges, definitions)
    }

    fn reading(count: u32, leans: &[(u32, u32)]) -> Vec<u32> {
        let (read, edges, definitions) = built(count, leans, &[]);
        order(&read, &edges, &definitions, &Grouping::default())
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

    /// 0 and 1 both depend on 2, so 2 is read first.
    #[test]
    fn a_shared_foundation_comes_first() {
        let reading = reading(3, &[(0, 2), (1, 2)]);

        assert_eq!(reading[0], 2);
    }

    /// Finishing one chain before starting the other keeps fewer things open.
    #[test]
    fn one_chain_is_finished_before_the_next_is_started() {
        let reading = reading(4, &[(0, 1), (2, 3)]);
        let chains: Vec<bool> = reading.iter().map(|id| *id < 2).collect();

        assert!(
            chains == vec![true, true, false, false] || chains == vec![false, false, true, true],
            "chains were interleaved: {reading:?}"
        );
    }

    /// A definition with no dependencies either way is read last: it can go anywhere, so
    /// it shouldn't interrupt a branch.
    #[test]
    fn a_foundation_is_read_first_and_what_stands_alone_last() {
        // 0 leans on 2, and 1 stands alone.
        assert_eq!(reading(3, &[(0, 2)]), vec![2, 0, 1]);
    }

    #[test]
    fn a_circle_is_read_with_one_thing_taken_on_faith() {
        let (read, edges, definitions) = built(3, &[(0, 1), (1, 2), (2, 0)], &[]);
        let ordering = order(&read, &edges, &definitions, &Grouping::default());

        assert_eq!(ordering.steps.len(), 3);
        assert_eq!(ordering.cost.taken_on_faith, 1);
    }

    #[test]
    fn nothing_is_taken_on_faith_when_it_doesnt_have_to_be() {
        let (read, edges, definitions) = built(4, &[(0, 1), (1, 2), (2, 3)], &[]);

        assert_eq!(
            order(&read, &edges, &definitions, &Grouping::default(),)
                .cost
                .taken_on_faith,
            0
        );
    }

    /// 1 sits in another file; with all else equal, the reading stays put.
    #[test]
    fn a_reading_would_rather_stay_in_one_file() {
        let (read, edges, definitions) = built(3, &[(0, 1), (0, 2)], &["a.rs", "b.rs", "a.rs"]);
        let ordering = order(&read, &edges, &definitions, &Grouping::default());

        assert_eq!(ordering.cost.jumps, 1);
    }

    #[test]
    fn a_chain_keeps_only_one_thing_in_mind_at_a_time() {
        let (read, edges, definitions) = built(5, &[(0, 1), (1, 2), (2, 3), (3, 4)], &[]);

        assert_eq!(
            order(&read, &edges, &definitions, &Grouping::default(),)
                .cost
                .peak_open,
            1
        );
    }

    #[test]
    fn unrelated_definitions_leave_nothing_open() {
        let (read, edges, definitions) = built(4, &[], &[]);

        assert_eq!(
            order(&read, &edges, &definitions, &Grouping::default(),)
                .cost
                .peak_open,
            0
        );
    }

    /// A container's header is read right before its contents, not before what they depend
    /// on. 1 sits inside 0 and depends on 2 in another file, so 0 waits for 2 too.
    #[test]
    fn a_containers_header_is_read_right_before_what_it_holds() {
        let (read, edges, mut definitions) = built(3, &[(1, 2)], &["a.rs", "a.rs", "b.rs"]);
        for definition in &mut definitions {
            let identity = definition.identity;
            let Sides::Kept { before, after } = &mut definition.sides else {
                unreachable!("built() keeps everything");
            };
            for side in [before, after] {
                if identity == Identity(0) {
                    side.role = Role::Container;
                }
                if identity == Identity(1) {
                    side.parent = Some(Locator {
                        scope: Vec::new(),
                        name: "d0".to_string(),
                    });
                }
            }
        }

        let reading: Vec<u32> = order(&read, &edges, &definitions, &Grouping::default())
            .steps
            .iter()
            .map(|step| step.definition.0)
            .collect();
        assert_eq!(reading, vec![2, 0, 1]);
    }
}
