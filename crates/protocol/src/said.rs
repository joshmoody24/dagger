//! Everything one reading comes to, as whoever reads it receives it.
//!
//! The other half of the protocol. Adapters speak the shapes next door; this is what dagger
//! itself says once they've all answered, and the page reads nothing but this.
//!
//! Written down as a type rather than assembled from a map at the last moment, because a
//! map has no shape to generate TypeScript from — so the page had to describe the outermost
//! layer by hand, which is the one place a renamed field doesn't fail anywhere. It just
//! arrives undefined.

use crate::Note;
use dagger_core::group::Grouping;
use dagger_core::model::{Definition, Identity};
use dagger_core::order::Ordering;
use dagger_core::review::Review;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct Said {
    /// Everything else talks in identities. Without these there's nothing to turn one back
    /// into a name, a file, or the text a reader came to see.
    pub definitions: Vec<Definition>,
    pub review: Review,
    pub ordering: Ordering,
    pub grouping: Grouping,
    pub notes: Vec<Note>,
}

impl Said {
    /// What to hand over, which is only what can be used.
    ///
    /// A reading knows about every definition in both snapshots — the fallback reading
    /// holds a copy of each file it covers, changed or not — and almost none of that can be
    /// shown. Handing it over anyway means the page carries the whole repository twice to
    /// draw a few dozen boxes, and carries it as entries it can't resolve: a change against
    /// an identity with no definition to go with it is a fact about nothing.
    pub fn of(
        definitions: &[Definition],
        mut review: Review,
        ordering: Ordering,
        grouping: Grouping,
        notes: Vec<Note>,
    ) -> Self {
        // The members, and the containers drawn around them.
        let shown: BTreeSet<Identity> = review.members.union(&review.context).copied().collect();

        review
            .changes
            .retain(|identity, _| shown.contains(identity));

        Said {
            definitions: definitions
                .iter()
                .filter(|definition| shown.contains(&definition.identity))
                .cloned()
                .collect(),
            review,
            ordering,
            grouping,
            notes,
        }
    }
}
