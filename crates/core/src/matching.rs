use crate::diagnostic::Diagnostic;
use crate::model::{Definition, Identity, Locator, Occurrence, Piece, Sides};
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
    // Checked first, because everything after this takes a name for an identity; a repeated
    // name would quietly match one twin and report the rest as added or removed.
    let mut diagnostics = checked(&before.occurrences);
    diagnostics.append(&mut checked(&after.occurrences));

    let pairs = pair_up(&before.occurrences, &after.occurrences);
    let definitions = build_definitions(pairs, before.occurrences, after.occurrences);

    let before_index = locator_index(&definitions, |sides| sides.before());
    let after_index = locator_index(&definitions, |sides| sides.after());

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

/// Sanity checks on what an extractor hands over. Extractor mistakes (a comment filed as
/// a declaration, a line claimed twice) otherwise surface far away, as a review that
/// reads oddly.
fn checked(occurrences: &[Occurrence]) -> Vec<Diagnostic> {
    let mut found = twins(occurrences);
    found.extend(occurrences.iter().filter_map(tangled));
    found
}

/// Whether a definition's pieces divide it up, or trip over each other.
fn tangled(occurrence: &Occurrence) -> Option<Diagnostic> {
    let mut pieces: Vec<&Piece> = occurrence.parts.values().flatten().collect();
    pieces.sort_by_key(|piece| piece.span.start);

    let over = pieces.windows(2).find(|pair| {
        // Pieces of different files sit in different files, so they can't overlap.
        pair[0].file == pair[1].file && pair[1].span.start < pair[0].span.end
    })?;

    Some(Diagnostic::Tangled {
        definition: occurrence.locator.clone(),
        at: over[1].span.start,
    })
}

/// Names more than one definition in a snapshot answers to.
fn twins(occurrences: &[Occurrence]) -> Vec<Diagnostic> {
    occurrences
        .iter()
        .fold(
            BTreeMap::<&Locator, usize>::new(),
            |mut times, occurrence| {
                *times.entry(&occurrence.locator).or_default() += 1;
                times
            },
        )
        .into_iter()
        .filter(|(_, times)| *times > 1)
        .map(|(locator, times)| Diagnostic::TwoOfOneName {
            locator: locator.clone(),
            times,
        })
        .collect()
}

/// Which after-occurrence, if any, each before-occurrence turned into.
fn pair_up(before: &[Occurrence], after: &[Occurrence]) -> Vec<Option<usize>> {
    // Reversed so the first of a name wins, since a later insert overwrites an earlier one.
    let after_by_locator: BTreeMap<&Locator, usize> = after
        .iter()
        .enumerate()
        .rev()
        .map(|(index, occurrence)| (&occurrence.locator, index))
        .collect();

    let mut pairs: Vec<Option<usize>> = before
        .iter()
        .map(|occurrence| after_by_locator.get(&occurrence.locator).copied())
        .collect();
    let mut taken = pairs
        .iter()
        .flatten()
        .fold(vec![false; after.len()], |mut taken, &index| {
            taken[index] = true;
            taken
        });

    rescue_renames(before, after, &mut pairs, &mut taken);
    pairs
}

/// Pairs up the leftovers by text similarity, most alike first. Requiring identical text
/// would catch almost nothing, since people rename and edit in the same commit.
fn rescue_renames(
    before: &[Occurrence],
    after: &[Occurrence],
    pairs: &mut [Option<usize>],
    taken: &mut [bool],
) {
    let earlier: Vec<usize> = (0..before.len())
        .filter(|&index| pairs[index].is_none())
        .collect();
    let later: Vec<usize> = (0..after.len()).filter(|&index| !taken[index]).collect();

    let mut candidates: Vec<(usize, usize, usize)> = earlier
        .iter()
        .flat_map(|&was| later.iter().map(move |&is| (was, is)))
        // A container and an item are never the same definition, however alike the
        // text; one that changed role would be drawn from whichever side was asked.
        .filter(|&(was, is)| before[was].role == after[is].role)
        .filter_map(|(was, is)| {
            let alike = likeness(&before[was], &after[is]);
            // Scaled to an integer so pairs sort without comparing floats.
            (alike >= ALIKE_ENOUGH).then_some(((alike * 1000.0) as usize, was, is))
        })
        .collect();

    // Best first, so the most convincing pair claims its halves before a weaker one can.
    candidates.sort_by(|a, b| b.cmp(a));
    for (_, was, is) in candidates {
        if pairs[was].is_none() && !taken[is] {
            pairs[was] = Some(is);
            taken[is] = true;
        }
    }
}

/// Shared lines over total lines, 0 to 1, like git's rename detection. Lines rather than
/// characters, so a reformat doesn't look like a rewrite and it's cheap for every pair.
fn likeness(before: &Occurrence, after: &Occurrence) -> f64 {
    let lines = |occurrence: &Occurrence| {
        occurrence
            .parts
            .keys()
            .filter_map(|&part| occurrence.text_of(part))
            .flat_map(|text| {
                text.lines()
                    .map(str::trim)
                    .filter(|line| !line.is_empty())
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            })
            .fold(BTreeMap::<String, usize>::new(), |mut counted, line| {
                *counted.entry(line).or_default() += 1;
                counted
            })
    };

    let (was, is) = (lines(before), lines(after));
    let held: usize = was.values().sum::<usize>() + is.values().sum::<usize>();
    if held == 0 {
        return 0.0;
    }

    let shared: usize = was
        .iter()
        .map(|(line, count)| *count.min(is.get(line).unwrap_or(&0)))
        .sum();

    2.0 * shared as f64 / held as f64
}

/// Half the lines in common: the threshold git uses for renames. A matter of taste.
const ALIKE_ENOUGH: f64 = 0.5;

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
    use crate::model::Span;

    #[test]
    fn pieces_that_cover_each_other_are_reported() {
        let mut occurrence = crate::testing::occurrence("one", &[]);
        occurrence.parts.insert(
            Part::Docs,
            vec![Piece {
                text: "/** One. */\nconst ".to_string(),
                span: Span { start: 0, end: 19 },
                line: 1,
                file: "money.ts".to_string(),
            }],
        );
        occurrence.parts.insert(
            Part::Type,
            vec![Piece {
                text: "const one = 1;".to_string(),
                span: Span { start: 13, end: 27 },
                line: 2,
                file: "money.ts".to_string(),
            }],
        );

        let said = checked(std::slice::from_ref(&occurrence));
        assert!(
            matches!(said.as_slice(), [Diagnostic::Tangled { at: 13, .. }]),
            "expected the overlap to be reported, got {said:?}"
        );
    }

    #[test]
    fn an_overlap_is_found_however_the_file_is_spelled() {
        let mut occurrence = crate::testing::occurrence("one", &[]);
        occurrence.file = "money.ts".to_string();
        occurrence.parts.insert(
            Part::Docs,
            vec![Piece {
                text: "/** One. */\nconst ".to_string(),
                span: Span { start: 0, end: 19 },
                line: 1,
                file: "money.ts".to_string(),
            }],
        );
        occurrence.parts.insert(
            Part::Type,
            vec![Piece {
                text: "const one = 1;".to_string(),
                span: Span { start: 13, end: 27 },
                line: 2,
                file: "money.ts".to_string(),
            }],
        );

        let said = checked(std::slice::from_ref(&occurrence));
        assert!(
            matches!(said.as_slice(), [Diagnostic::Tangled { at: 13, .. }]),
            "expected the overlap to be reported, got {said:?}"
        );
    }

    #[test]
    fn pieces_that_divide_a_definition_up_are_fine() {
        let mut occurrence = crate::testing::occurrence("one", &[]);
        occurrence.parts.insert(
            Part::Docs,
            vec![Piece {
                text: "/** One. */\n".to_string(),
                span: Span { start: 0, end: 12 },
                line: 1,
                file: "money.ts".to_string(),
            }],
        );
        occurrence.parts.insert(
            Part::Type,
            vec![Piece {
                text: "const one = 1;".to_string(),
                span: Span { start: 12, end: 26 },
                line: 2,
                file: "money.ts".to_string(),
            }],
        );

        assert!(checked(std::slice::from_ref(&occurrence)).is_empty());
    }
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
        assert!(edits.type_changed);
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

    /// Renaming and editing together is the usual case, so it must pair up as one definition.
    #[test]
    fn a_rename_with_an_edit_is_still_one_definition() {
        let before = "let sum = a + b;\nlog(sum);\nreturn sum;";
        let after = "let sum = a + b;\nlog(sum);\nreturn round(sum);";

        let changed = changes(
            vec![occurrence("addMoney", &[(Part::Body, before)])],
            vec![occurrence("plusMoney", &[(Part::Body, after)])],
        );
        let [Change::Kept(edits)] = changed.as_slice() else {
            panic!("expected the rename to be paired up, got {changed:?}");
        };

        assert!(edits.type_changed, "a new name is a new type");
        assert!(edits.changed(Part::Body));
    }

    /// Two things sharing no lines are two things, however tempting a pair they make.
    #[test]
    fn leftovers_that_read_nothing_alike_stay_apart() {
        let changes = changes(
            vec![occurrence(
                "parse",
                &[(Part::Body, "read(); decode(); done();")],
            )],
            vec![occurrence("render", &[(Part::Body, "paint(); flush();")])],
        );

        assert_eq!(changes, vec![Change::Removed, Change::Added]);
    }

    /// Identical leftovers pair up either way round; it makes no difference which.
    #[test]
    fn identical_leftovers_pair_up_rather_than_being_given_up_on() {
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

        assert!(
            changes
                .iter()
                .all(|change| matches!(change, Change::Kept(_))),
            "expected two renames, got {changes:?}"
        );
    }
}
