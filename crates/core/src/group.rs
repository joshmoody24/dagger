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
use std::collections::BTreeMap;

/// Where one definition sits, outermost group first. Empty means the grouping has nothing
/// to say about it, which is where anything unclaimed ends up.
pub type Path = Vec<String>;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct Grouping {
    /// What a reader would call this way of grouping: "package", "owner", "layer".
    pub name: String,
    pub of: BTreeMap<Identity, Path>,
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
}
