use crate::diagnostic::Diagnostic;
use crate::model::{Definition, Identity, Locator, Occurrence, Part, Piece, Sides};
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
    /* Checked before anything is done with them, because everything after this takes a
     * name for an identity. Left unsaid, a repeated name doesn't fail — it quietly matches
     * one of the twins and reports the others as arriving or leaving. */
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

/// What has to be true of what an extractor hands over, checked before anything leans on
/// it.
///
/// An extractor is the one piece of this that can't be written once and left alone: there's
/// a new one for every language, each reconstructing structure out of whatever its tools
/// happen to say. The mistakes they make are quiet — a comment filed as a declaration, a
/// line claimed twice — and they surface a long way from here, as a review that reads
/// oddly rather than as anything failing. Said plainly at the boundary, they're a line in a
/// panel instead of an afternoon.
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
        /* Pieces of different files sit in different files, so they can't overlap. Asked
         * through the occurrence, because a piece says which file it's in only when that
         * isn't the definition's own — so `None` and `Some(its own file)` are the same
         * place written two ways, and comparing them as written misses the overlap. */
        occurrence.home_of(pair[0]) == occurrence.home_of(pair[1])
            && pair[1].span.start < pair[0].span.end
    })?;

    Some(Diagnostic::Tangled {
        definition: occurrence.locator.clone(),
        at: over[1].span.start,
    })
}

/// Names more than one definition in a snapshot answers to.
fn twins(occurrences: &[Occurrence]) -> Vec<Diagnostic> {
    let mut times: BTreeMap<&Locator, usize> = BTreeMap::new();
    for occurrence in occurrences {
        *times.entry(&occurrence.locator).or_default() += 1;
    }

    times
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
    let mut after_by_locator: BTreeMap<&Locator, usize> = BTreeMap::new();
    for (index, occurrence) in after.iter().enumerate() {
        after_by_locator.entry(&occurrence.locator).or_insert(index);
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

/// Pairs up what's left over by how alike it reads.
///
/// Whatever the locators didn't match is either something that arrived, something that
/// left, or the same thing under a new name or in a new place. Text is the only evidence
/// left, so the most alike pair goes together, then the next, until nothing left is alike
/// enough to be worth claiming.
///
/// Insisting on identical text, which is what this did first, turned out to catch almost
/// nothing: people rename a thing and adjust it in the same breath, and a definition that
/// moved to another module usually picked up an edit on the way. Every rename in this
/// project's own history read as an arrival and a departure.
fn rescue_renames(
    before: &[Occurrence],
    after: &[Occurrence],
    pairs: &mut [Option<usize>],
    taken: &mut [bool],
) {
    let leftovers = |count: usize, used: &dyn Fn(usize) -> bool| {
        (0..count).filter(|index| !used(*index)).collect::<Vec<_>>()
    };
    let earlier = leftovers(before.len(), &|index| pairs[index].is_some());
    let later = leftovers(after.len(), &|index| taken[index]);

    let mut candidates: Vec<(usize, usize, usize)> = Vec::new();
    for &was in &earlier {
        for &is in &later {
            let alike = likeness(&before[was], &after[is]);
            if alike >= ALIKE_ENOUGH {
                // Scaled to an integer so pairs sort without comparing floats.
                candidates.push(((alike * 1000.0) as usize, was, is));
            }
        }
    }

    // Best first, so the most convincing pair claims its halves before a weaker one can.
    candidates.sort_by(|a, b| b.cmp(a));
    for (_, was, is) in candidates {
        if pairs[was].is_none() && !taken[is] {
            pairs[was] = Some(is);
            taken[is] = true;
        }
    }
}

/// How alike two definitions read, from nothing in common to word for word.
///
/// Lines shared over lines held, which is the same shape of measure version control uses
/// to spot a renamed file. Counting lines rather than characters keeps a reformatting from
/// looking like a rewrite, and keeps this cheap enough to run over every leftover pair.
fn likeness(before: &Occurrence, after: &Occurrence) -> f64 {
    let lines = |occurrence: &Occurrence| {
        let mut counted: BTreeMap<String, usize> = BTreeMap::new();
        for (_, text) in text_of(occurrence) {
            for line in text.lines() {
                let line = line.trim();
                if !line.is_empty() {
                    *counted.entry(line.to_string()).or_default() += 1;
                }
            }
        }
        counted
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

/// Half the lines in common. The one number in dagger that is a matter of taste rather
/// than a consequence of something: the same one git settled on for spotting a renamed
/// file, and for the same reason — below it, two things being related is a guess.
const ALIKE_ENOUGH: f64 = 0.5;

fn text_of(occurrence: &Occurrence) -> Vec<(Part, String)> {
    occurrence
        .parts
        .keys()
        .filter_map(|&part| occurrence.text_of(part).map(|text| (part, text)))
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
    use crate::model::Span;

    /* Each of these was a live bug in an extractor at some point, found by reading a
     * review and wondering. Said here, they'd have been a line in a panel. */
    #[test]
    fn pieces_that_cover_each_other_are_reported() {
        let mut occurrence = crate::testing::occurrence("one", &[]);
        occurrence.parts.insert(
            Part::Docs,
            vec![Piece {
                text: "/** One. */\nconst ".to_string(),
                span: Span { start: 0, end: 19 },
                line: 1,
                file: None,
            }],
        );
        occurrence.parts.insert(
            Part::Type,
            vec![Piece {
                text: "const one = 1;".to_string(),
                span: Span { start: 13, end: 27 },
                line: 2,
                file: None,
            }],
        );

        let said = checked(std::slice::from_ref(&occurrence));
        assert!(
            matches!(said.as_slice(), [Diagnostic::Tangled { at: 13, .. }]),
            "expected the overlap to be reported, got {said:?}"
        );
    }

    /* A piece names its file only when it isn't the definition's own, so a piece saying
     * nothing and one naming that same file are in the same place. Compared as written they
     * looked like different files, and pieces in different files can't overlap — so the
     * overlap went unreported. */
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
                file: None,
            }],
        );
        occurrence.parts.insert(
            Part::Type,
            vec![Piece {
                text: "const one = 1;".to_string(),
                span: Span { start: 13, end: 27 },
                line: 2,
                // The definition's own file, said out loud.
                file: Some("money.ts".to_string()),
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
                file: None,
            }],
        );
        occurrence.parts.insert(
            Part::Type,
            vec![Piece {
                text: "const one = 1;".to_string(),
                span: Span { start: 12, end: 26 },
                line: 2,
                file: None,
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

    /// Renaming and editing in the same breath is the usual way it happens, so this has to
    /// land as one definition that changed rather than two that came and went.
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

        assert!(edits.contract, "a new name is a new contract");
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

    /// Identical leftovers could pair up either way round, and it makes no difference: each
    /// pairing reads as the same two renames, with the same text on both sides.
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
