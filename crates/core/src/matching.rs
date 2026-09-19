use crate::diagnostic::Diagnostic;
use crate::model::{Definition, Identity, Locator, Occurrence, Part, Sides};
use crate::reference::{Mention, Reference, Site, Target};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// What an extractor reports about one snapshot.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Extraction {
    pub occurrences: Vec<Occurrence>,
    pub mentions: Vec<Mention>,
}

#[derive(Debug, Clone)]
pub struct Matched {
    pub definitions: Vec<Definition>,
    pub references: Vec<Reference>,
    pub diagnostics: Vec<Diagnostic>,
}

/// Decide which definitions in the two snapshots are the same definition, then say
/// everything else in terms of the identities that fall out.
pub fn match_snapshots(before: Extraction, after: Extraction) -> Matched {
    let pairs = pair_up(&before.occurrences, &after.occurrences);
    let definitions = build_definitions(pairs, before.occurrences, after.occurrences);

    let before_index = locator_index(&definitions, |sides| sides.before());
    let after_index = locator_index(&definitions, |sides| sides.after());

    let mut diagnostics = Vec::new();
    let references = collect_references(
        &before.mentions,
        &after.mentions,
        &before_index,
        &after_index,
        &mut diagnostics,
    );

    Matched {
        definitions,
        references,
        diagnostics,
    }
}

/// Which after-occurrence, if any, each before-occurrence turned into.
fn pair_up(before: &[Occurrence], after: &[Occurrence]) -> Vec<Option<usize>> {
    let mut after_by_locator: BTreeMap<&Locator, usize> = BTreeMap::new();
    for (index, occurrence) in after.iter().enumerate() {
        after_by_locator.insert(&occurrence.locator, index);
    }

    let mut taken = vec![false; after.len()];
    let mut pairs: Vec<Option<usize>> = before
        .iter()
        .map(|occurrence| {
            let found = after_by_locator.get(&occurrence.locator).copied();
            if let Some(index) = found {
                taken[index] = true;
            }
            found
        })
        .collect();

    rescue_renames(before, after, &mut pairs, &mut taken);
    pairs
}

/// Anything left over that reads exactly the same on both sides is the same thing
/// under a new name or in a new place. We only take the offer when it's unambiguous,
/// because two definitions with identical text give us no way to tell which is which.
fn rescue_renames(
    before: &[Occurrence],
    after: &[Occurrence],
    pairs: &mut [Option<usize>],
    taken: &mut [bool],
) {
    let leftovers = |occurrences: &[Occurrence], used: &[bool]| {
        let mut by_text: BTreeMap<Vec<(Part, String)>, Vec<usize>> = BTreeMap::new();
        for (index, occurrence) in occurrences.iter().enumerate() {
            if !used[index] {
                by_text.entry(text_of(occurrence)).or_default().push(index);
            }
        }
        by_text
    };

    let before_used: Vec<bool> = pairs.iter().map(|p| p.is_some()).collect();
    let before_leftovers = leftovers(before, &before_used);
    let after_leftovers = leftovers(after, taken);

    for (text, befores) in &before_leftovers {
        let Some(afters) = after_leftovers.get(text) else {
            continue;
        };
        if befores.len() != 1 || afters.len() != 1 {
            continue;
        }
        pairs[befores[0]] = Some(afters[0]);
        taken[afters[0]] = true;
    }
}

fn text_of(occurrence: &Occurrence) -> Vec<(Part, String)> {
    occurrence
        .parts
        .iter()
        .map(|(part, text)| (*part, text.text.clone()))
        .collect()
}

fn build_definitions(
    pairs: Vec<Option<usize>>,
    before: Vec<Occurrence>,
    after: Vec<Occurrence>,
) -> Vec<Definition> {
    let mut after: Vec<Option<Occurrence>> = after.into_iter().map(Some).collect();
    let paired: Vec<Sides> = before
        .into_iter()
        .zip(&pairs)
        .map(
            |(before, paired)| match paired.and_then(|index| after[index].take()) {
                Some(after) => Sides::Kept { before, after },
                None => Sides::Removed(before),
            },
        )
        .collect();

    paired
        .into_iter()
        .chain(after.into_iter().flatten().map(Sides::Added))
        .enumerate()
        .map(|(index, sides)| Definition {
            identity: Identity(index as u32),
            sides,
        })
        .collect()
}

fn locator_index(
    definitions: &[Definition],
    side: impl Fn(&Sides) -> Option<&Occurrence>,
) -> BTreeMap<Locator, Identity> {
    definitions
        .iter()
        .filter_map(|definition| {
            side(&definition.sides).map(|occ| (occ.locator.clone(), definition.identity))
        })
        .collect()
}

type Sites = (Vec<Site>, Vec<Site>);

/// Line the two snapshots' mentions up by the identities they landed on, so a call
/// that only shows up on one side reads as added or removed.
fn collect_references(
    before: &[Mention],
    after: &[Mention],
    before_index: &BTreeMap<Locator, Identity>,
    after_index: &BTreeMap<Locator, Identity>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<Reference> {
    let mut sites: BTreeMap<(Identity, Target<Identity>), Sites> = BTreeMap::new();

    for (mentions, index, is_before) in [(before, before_index, true), (after, after_index, false)]
    {
        for mention in mentions {
            let Some(from) = index.get(&mention.from).copied() else {
                diagnostics.push(Diagnostic::MentionFromNowhere {
                    from: mention.from.clone(),
                });
                continue;
            };
            let to = match &mention.to {
                Target::Known(locator) => match index.get(locator) {
                    Some(identity) => Target::Known(*identity),
                    None => Target::Unknown {
                        symbol: locator.name.clone(),
                    },
                },
                Target::Unknown { symbol } => Target::Unknown {
                    symbol: symbol.clone(),
                },
            };

            let entry = sites.entry((from, to)).or_default();
            let side = if is_before {
                &mut entry.0
            } else {
                &mut entry.1
            };
            side.push(mention.site.clone());
        }
    }

    sites
        .into_iter()
        .map(|((from, to), (before, after))| Reference {
            from,
            to,
            before,
            after,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::change::{Change, classify};
    use crate::model::Part;
    use crate::testing::occurrence;

    fn extraction(occurrences: Vec<Occurrence>) -> Extraction {
        Extraction {
            occurrences,
            mentions: Vec::new(),
        }
    }

    fn changes(before: Vec<Occurrence>, after: Vec<Occurrence>) -> Vec<Change> {
        match_snapshots(extraction(before), extraction(after))
            .definitions
            .iter()
            .map(|definition| classify(definition).0)
            .collect()
    }

    #[test]
    fn the_same_locator_on_both_sides_is_one_definition() {
        let changes = changes(
            vec![occurrence("zero", &[(Part::Body, "old")])],
            vec![occurrence("zero", &[(Part::Body, "new")])],
        );

        assert!(matches!(changes.as_slice(), [Change::Kept(_)]));
    }

    #[test]
    fn a_locator_on_one_side_only_is_added_or_removed() {
        let changes = changes(
            vec![occurrence("ZERO", &[(Part::Body, "= 0")])],
            vec![occurrence("SUPPORTED", &[(Part::Body, "= new Set()")])],
        );

        assert_eq!(changes, vec![Change::Removed, Change::Added]);
    }

    /// Same text, new name, and nothing else to confuse it with.
    #[test]
    fn a_rename_is_caught_when_the_text_is_untouched() {
        let changes = changes(
            vec![occurrence("addMoney", &[(Part::Body, "a + b")])],
            vec![occurrence("plusMoney", &[(Part::Body, "a + b")])],
        );

        let [Change::Kept(edits)] = changes.as_slice() else {
            panic!("expected one kept definition, got {changes:?}");
        };
        assert!(edits.contract);
        assert!(!edits.changed(Part::Body));
    }

    #[test]
    fn a_move_is_caught_when_the_text_is_untouched() {
        let mut moved = occurrence("zero", &[(Part::Body, "return 0")]);
        moved.file = "amount.ts".to_string();

        let changes = changes(
            vec![occurrence("zero", &[(Part::Body, "return 0")])],
            vec![moved],
        );

        let [Change::Kept(edits)] = changes.as_slice() else {
            panic!("expected one kept definition, got {changes:?}");
        };
        assert!(edits.moved);
        assert!(!edits.worth_reading());
    }

    /// Editing while renaming puts it beyond what we're willing to guess at.
    #[test]
    fn a_rename_with_an_edit_reads_as_add_and_remove() {
        let changes = changes(
            vec![occurrence("addMoney", &[(Part::Body, "a + b")])],
            vec![occurrence("plusMoney", &[(Part::Body, "a + b + 1")])],
        );

        assert_eq!(changes, vec![Change::Removed, Change::Added]);
    }

    /// Two identical leftovers give us no way to tell which became which.
    #[test]
    fn identical_leftovers_are_left_alone() {
        let changes = changes(
            vec![
                occurrence("one", &[(Part::Body, "noop")]),
                occurrence("two", &[(Part::Body, "noop")]),
            ],
            vec![
                occurrence("three", &[(Part::Body, "noop")]),
                occurrence("four", &[(Part::Body, "noop")]),
            ],
        );

        assert_eq!(
            changes,
            vec![
                Change::Removed,
                Change::Removed,
                Change::Added,
                Change::Added
            ]
        );
    }
}
