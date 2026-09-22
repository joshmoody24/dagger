//! The order to read a change in.
//!
//! One rule above all others: don't read something before the things it leans on. After
//! that, finish where you are before going elsewhere, and meet what everything stands on
//! before the things standing on it. A reader led back and forth across a change spends
//! their attention on finding their place rather than on the code.
//!
//! Something is *open* once it's been read and something still unread depends on it:
//! it has to be kept in mind. It closes when the last thing depending on it is read,
//! which is the satisfying part of finishing a branch. Something taken *on faith* is
//! the opposite, a promise held about code not yet seen, which only happens when
//! definitions depend on each other in a circle and there's no honest place to start.
//!
//! Candidates are compared in order rather than scored and added up, so there are no
//! weights to argue about — `pick` lists the order, a line of reasoning each.
//!
//! There was for a while a second reading that left every foundation until the last
//! moment, to carry as little as possible at once. Measured against this one it held no
//! less in mind at the worst moment, moved the reader between packages half again as
//! often, and began in whichever package the change happened to lean on hardest — which
//! on a page laid out by what depends on what is the bottom. It's gone; what's left is
//! the one reading, and the numbers it costs are in `Cost` for arguing with.

use crate::group::Grouping;
use crate::model::{self, Identity, Locator, Role};
use crate::review::Edge;
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

/// `read` is what's worth reading; `edges` is what leans on what among everything drawn.
///
/// Takes the two things it uses rather than the whole review, which is what let the review
/// be one object: a function asking for more than it needs was the only reason the analysis
/// had to exist as a separate type before the reading could be worked out from it.
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
    /* Whatever holds others says what it is before the things it holds: a module's prose
     * and imports, an impl's header. Read afterwards, it arrives once nobody needs it. */
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

    // Where each one sits, for judging whether reading the next takes the reader somewhere
    // else. A grouping says where if it has an opinion; failing that, the file it's in.
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
    /* Worked out once, by whoever knows how the groups sit, and read here. The page reads
     * the same numbers, which is what keeps a reading running down it. */
    let bands: Vec<u32> = members
        .iter()
        .map(|identity| grouping.band_of(*identity))
        .collect();

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
                bands: &bands,
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

/// Which members lean on which, and the same the other way round.
///
/// A container leans on whatever its contents lean on outside it. Its own definition is its
/// prose and imports — what it brings in is exactly what it stands on — but nothing ever
/// says so as an edge, so it stood on nothing. "Never before what it leans on" then let a
/// module through the moment the reader arrived, while everything it holds was still
/// waiting on other modules: its header read seven steps ahead of the first line it holds,
/// introducing nothing.
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
        /* Outward from the leaner, stopping at the first container that holds what it
         * leans on too — everything further out holds both, and a thing doesn't stand on
         * what's inside it. */
        for &container in around(&edge.from) {
            if container == edge.to || around(&edge.to).contains(&container) {
                break;
            }
            lean(container, edge.to);
        }
    }

    (leans_on, holds_up)
}

/// What each definition is written inside, nearest first, as far as that stays in the same
/// file. A parent is said as a name, so it's resolved by name — within the file, since a
/// name means nothing outside the one that holds it.
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

/// The module each member belongs to, named so two can be told apart.
///
/// A module, not a file. They line up most of the time, which is why the file alone would
/// nearly work — but a file holding two modules holds two trains of thought, and the reader
/// knows it even when the filesystem doesn't.
///
/// The outermost container a file holds is that file's module, whatever the language calls
/// it, so this needs to know nothing about modules to find one. Stopping at the first
/// container instead would make a method and a plain function in one module two places,
/// and a reader moving between them has not gone anywhere.
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

    // A file can't hold a null and neither can a name, so the two can't be confused.
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

/// Where the reader is, and where everything else is, at every sense of the word.
struct Where<'a> {
    groups: &'a [&'a [String]],
    homes: &'a [String],
    bands: &'a [u32],
    group: Option<&'a [String]>,
    home: Option<&'a str>,
}

impl Where<'_> {
    /// How far reading this one would take the reader: nowhere, out of the module, or out
    /// of the package altogether. Counting only the package let a reading wander between
    /// the modules inside one as freely as if they were the same place, which to whoever
    /// is reading them they are not.
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
                // Never before what it leans on.
                unread_leans[candidate],
                // Stay in the module you're in: a reader pulled out of one has to come
                // back to it later and find the thread again.
                at.away(candidate),
                // Then which group. Whatever leans on no other group is read before what
                // leans on it, which is the order the page draws them in from the top. At
                // the first step there's nowhere to stay, so this decides where to begin,
                // and the page puts whatever is read first at the top left.
                at.bands[candidate] as isize,
                // Arriving somewhere, read what it says it is before what it holds: a
                // module's own definition is its file's prose and what it brings in.
                //
                // Costs more held in mind on some readings and less on others — a module's
                // imports count against it the way a function's contract does, which
                // overstates them, since nobody holds an import list in their head.
                isize::from(!headers[candidate]),
                // Then finish a branch, when one can be finished.
                -(closes as isize),
                // Then, among things equally free to read, whatever the most is waiting
                // on — which is what "upstream" means once the names are taken away.
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

    /// `count` definitions, all edited, wired up by the given pairs. Each pair reads
    /// "the first leans on the second".
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

    /// What something stands on is read before it, and the branch is finished before
    /// anything else is begun — so a definition that stands alone, owing nothing and owed
    /// nothing, is read last rather than first. It can be read at any time, which is
    /// exactly why it shouldn't interrupt something that can't.
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
            order(&read, &edges, &definitions, &Grouping::default())
                .cost
                .taken_on_faith,
            0
        );
    }

    /// Same graph, but 1 sits in another file. Since either order is otherwise equal,
    /// the reading should stay put rather than hop out and back.
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
            order(&read, &edges, &definitions, &Grouping::default())
                .cost
                .peak_open,
            1
        );
    }

    #[test]
    fn unrelated_definitions_leave_nothing_open() {
        let (read, edges, definitions) = built(4, &[], &[]);

        assert_eq!(
            order(&read, &edges, &definitions, &Grouping::default())
                .cost
                .peak_open,
            0
        );
    }

    /// A module's header — its own prose and imports — is read on arriving at the module,
    /// and not before: not ahead of what its contents lean on, which put the introduction
    /// steps before the thing it introduces. 1 sits inside 0 and leans on 2, which is
    /// elsewhere — so 0 has to wait for 2 just as 1 does, and then comes right before 1.
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
