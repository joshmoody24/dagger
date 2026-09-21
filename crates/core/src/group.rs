//! Which group each definition belongs to, and what that's for.
//!
//! A group is a box a reader can be drawn around several definitions: a bazel package, a
//! crate, a workspace, whoever owns the code. Groups nest, so a group is named by a path
//! from the outermost inwards.
//!
//! Only one grouping is in use at a time. Boxes have to nest to be drawn, and two ways of
//! grouping the same definitions rarely nest inside each other — a package and an owner cut
//! across one another. Rather than trying to draw both, a reader picks which one they're
//! looking through, and the review is arranged along that one.

use crate::model::Identity;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Where one definition sits, outermost group first. Empty means the grouping has nothing
/// to say about it, which is where anything unclaimed ends up.
pub type Path = Vec<String>;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct Grouping {
    /// What a reader would call this way of grouping: "package", "owner", "layer".
    pub name: String,
    pub of: BTreeMap<Identity, Path>,
    /// How deep each group sits among the groups: nought for one that leans on no other,
    /// one more than the furthest it leans on otherwise. Keyed the way a group is written
    /// on a page, outermost first, joined by slashes.
    ///
    /// Worked out here so it's worked out once. The reading follows it — what leans on no
    /// other group is read before what leans on it — and the page draws its rows of boxes
    /// by it, and those two being the same number is the whole reason a reading runs down
    /// a page rather than around it. Two of them would agree until they didn't, and the
    /// symptom would be an order that feels random.
    #[serde(default)]
    pub bands: BTreeMap<String, u32>,
}

impl Grouping {
    pub fn path_of(&self, definition: Identity) -> &[String] {
        self.of.get(&definition).map(Vec::as_slice).unwrap_or(&[])
    }

    /// Whether two definitions sit in the same group. What the reading order asks, to know
    /// whether moving from one to the next takes the reader somewhere else.
    pub fn together(&self, one: Identity, other: Identity) -> bool {
        self.path_of(one) == self.path_of(other)
    }

    /// How deep a definition's group sits. Ungrouped things are at the top, having nothing
    /// to sit behind.
    pub fn band_of(&self, definition: Identity) -> u32 {
        self.bands
            .get(&named(self.path_of(definition)))
            .copied()
            .unwrap_or(0)
    }

    /// Works out how the groups sit relative to each other, from what leans on what.
    ///
    /// Done as a step of its own rather than while grouping, because it needs something
    /// grouping doesn't have: the edges, which aren't known until the review is.
    pub fn settle(&mut self, edges: &[(Identity, Identity)]) {
        let mut between: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for path in self.of.values() {
            between.entry(named(path)).or_default();
        }
        for (from, to) in edges {
            let (from, to) = (named(self.path_of(*from)), named(self.path_of(*to)));
            if from != to {
                between.entry(from).or_default().insert(to);
            }
        }

        let mut deep = BTreeMap::new();
        for group in between.keys() {
            depth(group, &between, &mut deep);
        }
        self.bands = deep;
    }
}

/// A group path as the page writes it, which is how the bands are keyed.
fn named(path: &[String]) -> String {
    path.join("/")
}

/// One more than the furthest thing it leans on. A circle is settled by whoever is asked
/// first, which is enough: being in one means there's no right answer, only a readable one.
fn depth(
    group: &str,
    between: &BTreeMap<String, BTreeSet<String>>,
    seen: &mut BTreeMap<String, u32>,
) -> u32 {
    if let Some(&found) = seen.get(group) {
        return found;
    }
    seen.insert(group.to_string(), 0);
    let found = between
        .get(group)
        .into_iter()
        .flatten()
        .filter(|other| other.as_str() != group)
        .map(|other| depth(other, between, seen) + 1)
        .max()
        .unwrap_or(0);
    seen.insert(group.to_string(), found);
    found
}
