//! Which group each definition belongs to.
//!
//! A group is a box drawn around several definitions: a package, a crate, an owner. Only
//! one grouping is in use at a time, since two groupings rarely nest inside each other.

use crate::model::Identity;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Where one definition sits, outermost group first. Empty means it's in no group.
pub type GroupPath = Vec<String>;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct Grouping {
    /// What a reader would call this way of grouping: "package", "owner", "layer".
    pub name: String,
    pub of: BTreeMap<Identity, GroupPath>,
}

impl Grouping {
    pub fn path_of(&self, definition: Identity) -> &[String] {
        self.of.get(&definition).map(Vec::as_slice).unwrap_or(&[])
    }
}
