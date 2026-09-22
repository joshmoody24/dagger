//! Which files are still worth asking about, and what for.
//!
//! Shared because the walk itself has nothing to do with any one language: it's a queue of
//! files, how far each sits from a change, and which of a file's definitions have earned
//! being asked about — by a byte range touching them, or by name, because a break travels
//! through them. Both adapters that speak to a language server over this crate's transport
//! walk out from a change the same way, so this is worked out once rather than twice.

use dagger_core::model::{Locator, Span};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::ops::Range;

/// Every byte in a span, which as a sentinel for "the whole file" makes the overlap
/// check in [`Wanted::covers`] true for anything: nothing starts past it, and nothing
/// ends before it.
pub const EVERYTHING: Range<usize> = 0..usize::MAX;

/// What's wanted from one file: byte ranges known to have changed, and definitions
/// named directly because something that referred to them broke.
///
/// One shape for both reasons a file ends up on the frontier, because both answer the
/// same question — which definitions are worth asking about — and a definition can earn
/// its place either way, or both. A changed file seeds `ranges`; a file reached by
/// following a break adds one name to `names`; nothing here ever means "ask about
/// everything the file happens to define".
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Wanted {
    ranges: Vec<Range<usize>>,
    names: BTreeSet<Locator>,
}

impl Wanted {
    /// Everything in the file, told without knowing where. What a changed file wants
    /// when there's nothing narrower to say — a binary file, say, where a line diff has
    /// nothing to report — the same as every changed file wanted before ranges existed.
    pub fn everything() -> Self {
        Wanted {
            ranges: vec![EVERYTHING],
            names: BTreeSet::new(),
        }
    }

    /// What a changed file wants, from wherever dagger said it differs. Empty spans mean
    /// the same as no narrower answer at all: the whole file.
    pub fn from_spans(spans: &[Span]) -> Self {
        if spans.is_empty() {
            return Self::everything();
        }
        Wanted {
            ranges: spans
                .iter()
                .map(|span| span.start as usize..span.end as usize)
                .collect(),
            names: BTreeSet::new(),
        }
    }

    pub fn named(locator: Locator) -> Self {
        Wanted {
            ranges: Vec::new(),
            names: BTreeSet::from([locator]),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.ranges.is_empty() && self.names.is_empty()
    }

    /// Whether a definition earns being asked about: its own span overlaps something
    /// that changed, or it was named outright.
    pub fn covers(&self, whole: &Range<usize>, name: &Locator) -> bool {
        self.names.contains(name)
            || self
                .ranges
                .iter()
                .any(|range| range.start < whole.end && whole.start < range.end)
    }
}

/// One file's place on the frontier: how far out it sits, and what's been asked about
/// versus what's still waiting to be.
#[derive(Default)]
struct FileWork {
    away: u32,
    asked: Wanted,
    pending: Wanted,
}

/* Which files are still to be walked, what each is wanted for, and how much has been done.
 *
 * A file earns its place here once, however many times it turns up: one that uses a changed
 * definition in twenty signatures is twenty answers from the server and one file to walk.
 * Letting those through put twenty copies on the queue, which cost nothing to skip later but
 * made the count of what's left meaningless — it went up and down as copies drained.
 */
#[derive(Default)]
pub struct Frontier {
    queue: VecDeque<String>,
    files: BTreeMap<String, FileWork>,
}

impl Frontier {
    /// Says a file wants asking about, and what for. Anything already asked is dropped
    /// here rather than discovered again later; anything already waiting is folded into
    /// what's already pending rather than queued a second time.
    pub fn want(&mut self, path: &str, wanted: Wanted, away: u32) {
        let work = self
            .files
            .entry(path.to_string())
            .or_insert_with(|| FileWork {
                away,
                ..FileWork::default()
            });
        work.away = work.away.min(away);

        // Ranges are seeded exactly once, when a file is first found to have changed —
        // nothing later ever adds more — so once any are asked or waiting, there's
        // nothing fresh to add.
        let has_ranges = |wanted: &Wanted| !wanted.ranges.is_empty();
        let fresh_ranges = match has_ranges(&work.asked) || has_ranges(&work.pending) {
            true => Vec::new(),
            false => wanted.ranges,
        };

        /* A whole file already wants asking about everything in it, which is every name
         * there is — so a name arriving after covers nothing new, and letting it in would
         * walk the file a second time to ask about something already covered the first. */
        let covers_everything = |wanted: &Wanted| wanted.ranges.contains(&EVERYTHING);
        let fresh_names: BTreeSet<Locator> = if covers_everything(&work.asked)
            || covers_everything(&work.pending)
            || fresh_ranges.contains(&EVERYTHING)
        {
            BTreeSet::new()
        } else {
            wanted
                .names
                .difference(&work.asked.names)
                .cloned()
                .collect()
        };

        if fresh_ranges.is_empty() && fresh_names.is_empty() {
            return;
        }

        let was_idle = work.pending.is_empty();
        work.pending.ranges.extend(fresh_ranges);
        work.pending.names.extend(fresh_names);
        if was_idle {
            self.queue.push_back(path.to_string());
        }
    }

    pub fn waiting(&self) -> Vec<String> {
        self.queue.iter().cloned().collect()
    }

    /// The next file to walk, taken off the queue and marked as asked for what it was
    /// wanted for, so it can come round again only for something new.
    ///
    /// Not an iterator, though it looks like one: walking a file is what turns up the next
    /// files worth walking, so the queue grows while it's being drained — a second borrow
    /// that no `for` loop allows.
    pub fn take(&mut self) -> Option<(String, Wanted, u32)> {
        let path = self.queue.pop_front()?;
        let work = self.files.get_mut(&path)?;
        let wanted = std::mem::take(&mut work.pending);
        work.asked.ranges.extend(wanted.ranges.iter().cloned());
        work.asked.names.extend(wanted.names.iter().cloned());
        Some((path, wanted, work.away))
    }

    /// How many files have been walked, and how many are known to want walking.
    ///
    /// Files, not visits: one already walked comes round again when something new in it
    /// turns out to carry a break, and counting those made the walk look further along
    /// than the thing it was counting towards.
    pub fn walked(&self) -> usize {
        self.files
            .values()
            .filter(|work| !work.asked.is_empty())
            .count()
    }

    pub fn known(&self) -> usize {
        self.files.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn named(name: &str) -> Locator {
        Locator {
            scope: Vec::new(),
            name: name.to_string(),
        }
    }

    fn just(names: &[&str]) -> Wanted {
        Wanted {
            ranges: Vec::new(),
            names: names.iter().map(|name| named(name)).collect(),
        }
    }

    /* The narrowing item 4 exists for: a changed file used to mean every definition in it
     * was asked about, hover and references alike, whether or not that definition's own
     * text had moved. Now only what a changed range actually touches earns the question. */
    #[test]
    #[allow(clippy::single_range_in_vec_init)]
    fn a_definition_is_covered_only_where_a_range_touches_its_own_span() {
        let touched = Wanted {
            ranges: vec![40..60],
            names: BTreeSet::new(),
        };
        assert!(touched.covers(&(50..70), &named("inside")));
        assert!(!touched.covers(&(100..120), &named("elsewhere")));
    }

    /// Sharing an edge isn't overlapping it — a range that ends exactly where a
    /// definition begins hasn't touched anything the definition owns.
    #[test]
    #[allow(clippy::single_range_in_vec_init)]
    fn touching_at_the_edge_is_not_covering() {
        let wanted = Wanted {
            ranges: vec![0..10],
            names: BTreeSet::new(),
        };
        assert!(!wanted.covers(&(10..20), &named("after")));
    }

    /// Named outright always counts, whatever the ranges say — this is how a break
    /// reaches a definition whose own text never moved.
    #[test]
    fn a_named_definition_is_covered_with_no_ranges_at_all() {
        let wanted = just(&["carries"]);
        assert!(wanted.covers(&(0..5), &named("carries")));
        assert!(!wanted.covers(&(0..5), &named("someone_else")));
    }

    #[test]
    fn everything_covers_any_span_at_all() {
        let wanted = Wanted::everything();
        assert!(wanted.covers(&(0..1), &named("first")));
        assert!(wanted.covers(&(1_000_000..1_000_010), &named("far away")));
    }

    #[test]
    fn spans_narrow_to_only_what_they_cover() {
        let wanted = Wanted::from_spans(&[Span { start: 10, end: 20 }]);
        assert!(wanted.covers(&(12..15), &named("inside")));
        assert!(!wanted.covers(&(100..200), &named("elsewhere")));
    }

    /* A file dagger says differs but a line diff has nothing narrower to say about —
     * binary content is the usual reason — falls back to wanting the whole thing.
     * Silently asking about nothing would be wrong, not just slow. */
    #[test]
    fn no_spans_at_all_wants_the_whole_thing() {
        let wanted = Wanted::from_spans(&[]);
        assert!(wanted.covers(&(0..1), &named("anything")));
        assert!(wanted.covers(&(9_999..10_000), &named("anything else")));
    }

    #[test]
    fn a_file_is_walked_once_however_often_it_turns_up() {
        let mut front = Frontier::default();
        front.want("a.ts", Wanted::everything(), 0);
        for _ in 0..20 {
            front.want("b.ts", just(&["one"]), 0);
        }

        let mut walked = Vec::new();
        while let Some((path, _, _)) = front.take() {
            walked.push(path);
        }
        assert_eq!(walked, vec!["a.ts", "b.ts"]);
        assert_eq!(front.walked(), 2);
    }

    /* What's been done can't be more than what there is to do, however many times a file
     * comes round again for something new in it. */
    #[test]
    fn what_is_walked_never_outruns_what_is_known() {
        let mut front = Frontier::default();
        front.want("a.ts", just(&["one"]), 0);

        for step in 0..5 {
            front.take();
            let more = format!("more{step}");
            front.want("a.ts", just(&[&more]), 0);
            assert!(
                front.walked() <= front.known(),
                "walked {} of {}",
                front.walked(),
                front.known()
            );
        }
    }

    /* The whole point of carrying a name along: a file reached by following a break is
     * there for one definition, and asking after the rest is a search of the repository
     * for every other thing that happens to share the file. */
    #[test]
    fn a_file_reached_by_a_break_is_asked_only_about_what_carried_it() {
        let mut front = Frontier::default();
        front.want("b.ts", just(&["carries"]), 0);

        let (path, wanted, _) = front.take().expect("something to walk");
        assert_eq!(path, "b.ts");
        assert_eq!(wanted, just(&["carries"]));
    }

    /* Two definitions in one file can each carry a break, and both want asking about. */
    #[test]
    fn what_several_breaks_want_is_gathered_into_one_visit() {
        let mut front = Frontier::default();
        front.want("b.ts", just(&["one"]), 0);
        front.want("b.ts", just(&["two"]), 0);

        assert_eq!(front.take().unwrap().1, just(&["one", "two"]));
        assert_eq!(front.take(), None, "one file, one visit");
    }

    /* A changed file is read whole, and a name arriving after says nothing new — every
     * name is already covered. Ranges are always seeded before any name-based want can
     * happen, since a walk seeds every changed file before the walk that produces names
     * even starts, so this is the order a real walk ever sees. */
    #[test]
    fn wanting_everything_leaves_nothing_for_a_name_to_add() {
        let mut front = Frontier::default();
        front.want("a.ts", Wanted::everything(), 0);
        front.want("a.ts", just(&["one"]), 0);
        assert_eq!(front.take().unwrap().1, Wanted::everything());
    }

    #[test]
    fn a_file_already_walked_is_never_asked_the_same_thing_twice() {
        let mut front = Frontier::default();
        front.want("a.ts", just(&["one"]), 0);
        assert_eq!(front.take().unwrap().1, just(&["one"]));

        front.want("a.ts", just(&["one"]), 0);
        assert_eq!(
            front.take(),
            None,
            "asked again for what it already answered"
        );
    }

    /* But something genuinely new in a file already visited still gets asked. */
    #[test]
    fn a_file_already_walked_is_revisited_for_something_new() {
        let mut front = Frontier::default();
        front.want("a.ts", just(&["one"]), 0);
        front.take();

        front.want("a.ts", just(&["one", "two"]), 0);
        assert_eq!(
            front.take().unwrap().1,
            just(&["two"]),
            "should ask only for the part it hasn't"
        );
    }

    /* Reached two ways round, a file is as near as the nearest way to it — which is what
     * decides whether the walk carries on through it. */
    #[test]
    fn a_file_keeps_the_shortest_distance_found_to_it() {
        let mut front = Frontier::default();
        front.want("a.ts", just(&["one"]), 3);
        front.want("a.ts", just(&["two"]), 1);

        assert_eq!(front.take().unwrap().2, 1);
    }

    #[test]
    fn nothing_more_is_wanted_of_a_file_read_whole() {
        let mut front = Frontier::default();
        front.want("a.ts", Wanted::everything(), 0);
        front.take();

        front.want("a.ts", just(&["anything"]), 0);
        assert_eq!(front.take(), None);
    }
}
