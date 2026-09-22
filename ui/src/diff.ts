import type { Line, Shown } from "./dagger.ts";

/* Comparing two versions of a definition's text, line by line. Kept apart from the rest of
 * the page's work because it's the one part worth guarding for speed: a definition can be
 * a whole file, and a file can be six thousand lines long.
 */

/* How big a table of lines against lines is worth building. A hundred against a hundred is
 * instant; six thousand against six thousand is not. */
const EXACT = 40_000;

/** Line by line, marked as kept, gone, or new. */
export function compare(before: Line[] | null, after: Line[] | null): Shown[] {
  return diffing(before ?? [], after ?? []);
}

/* Lines against lines.
 *
 * The exact answer — the longest run of lines both sides share — costs a table of every
 * line against every other. That's fine until a definition is a whole file: six thousand
 * lines against six thousand is thirty-eight million cells, built to find sixteen changed
 * lines, and the page stops answering while it counts them.
 *
 * So the easy agreements are taken first. Matching ends line up and can't be anything else.
 * Then lines that appear exactly once on each side: a line with one home in each version
 * can only be that same line, wherever it has moved to. What's left between those is small,
 * and the exact answer is cheap on small things.
 */
function diffing(a: Line[], b: Line[]): Shown[] {
  let head = 0;
  while (head < a.length && head < b.length && a[head].text === b[head].text)
    head++;

  let tail = 0;
  while (
    tail < a.length - head &&
    tail < b.length - head &&
    a[a.length - 1 - tail].text === b[b.length - 1 - tail].text
  ) {
    tail++;
  }

  const kept = (lines: Line[]): Shown[] =>
    lines.map((line) => ({ mark: " " as const, line }));
  const [x, y] = [
    a.slice(head, a.length - tail),
    b.slice(head, b.length - tail),
  ];
  const middle = x.length * y.length <= EXACT ? exactly(x, y) : split(x, y);

  return [
    ...kept(b.slice(0, head)),
    ...middle,
    ...kept(b.slice(b.length - tail)),
  ];
}

/* Split around the lines that can only be themselves, and work on what's between them.
 *
 * A line appearing exactly once in each version is a place the two certainly meet, whatever
 * happened around it. Taking those as fixed turns one enormous comparison into many small
 * ones — and where there are none to be had, there's nothing for it but the table.
 */
function split(a: Line[], b: Line[]): Shown[] {
  const counted = (lines: Line[]) => {
    const seen = new Map<string, number>();
    for (const line of lines)
      seen.set(line.text, (seen.get(line.text) ?? 0) + 1);
    return seen;
  };
  const [inA, inB] = [counted(a), counted(b)];

  const whereB = new Map<string, number>();
  b.forEach((line, at) => {
    if (inB.get(line.text) === 1) whereB.set(line.text, at);
  });

  const pairs: [number, number][] = [];
  a.forEach((line, at) => {
    const there = whereB.get(line.text);
    if (inA.get(line.text) === 1 && there !== undefined)
      pairs.push([at, there]);
  });

  const anchors = rising(pairs);
  if (!anchors.length) return exactly(a, b);

  const shown: Shown[] = [];
  let [i, j] = [0, 0];
  for (const [x, y] of anchors) {
    shown.push(...diffing(a.slice(i, x), b.slice(j, y)));
    shown.push({ mark: " ", line: b[y] });
    [i, j] = [x + 1, y + 1];
  }
  shown.push(...diffing(a.slice(i), b.slice(j)));
  return shown;
}

/* Lined up where they sit, when there's nothing to line them up by.
 *
 * Last resort, for a stretch too big to weigh line against line and with no line in it
 * distinctive enough to anchor on. Comparing position against position is the one thing
 * left that's honest: where two versions of a repeated structure agree at a spot they are
 * almost certainly the same line, and where they don't the reader is shown both. Not the
 * shortest answer, but a true one, and it costs a single pass.
 */
function abreast(a: Line[], b: Line[]): Shown[] {
  const shown: Shown[] = [];
  for (let at = 0; at < Math.max(a.length, b.length); at++) {
    const [was, is] = [a[at], b[at]];
    if (was && is && was.text === is.text) {
      shown.push({ mark: " ", line: is });
      continue;
    }
    /* Gone before arrived, so reading past the additions still gives back the older
     * version and reading past the removals the newer. */
    if (was) shown.push({ mark: "−", line: was });
    if (is) shown.push({ mark: "+", line: is });
  }
  return shown;
}

/* The longest run of meeting points that moves forwards on both sides.
 *
 * Lines that meet in both versions can still have swapped places, and a pair that goes
 * backwards would have the diff crossing over itself. The longest run that doesn't is the
 * most of the file that can be left alone.
 */
function rising(pairs: [number, number][]): [number, number][] {
  const ends: number[] = [];
  const before: number[] = [];

  for (let at = 0; at < pairs.length; at++) {
    const [, y] = pairs[at];
    let low = 0;
    let high = ends.length;
    while (low < high) {
      const mid = (low + high) >> 1;
      if (pairs[ends[mid]][1] < y) low = mid + 1;
      else high = mid;
    }
    before[at] = low > 0 ? ends[low - 1] : -1;
    ends[low] = at;
  }

  const found: [number, number][] = [];
  let at = ends.length ? ends[ends.length - 1] : -1;
  while (at >= 0) {
    found.push(pairs[at]);
    at = before[at]!;
  }
  return found.reverse();
}

/* Every line against every other: the exact answer, for when there's little enough left to
 * ask for it. This is what the whole comparison used to be.
 *
 * Guarded here rather than at each place it's called, so nothing can reach the table by a
 * route that forgot to check. Nothing should arrive too big — the ends are matched off
 * first and the middle split at lines that can only be themselves — but a stretch with no
 * line appearing exactly once on each side has nothing to split on, and a file of repeated
 * punctuation is exactly that. */
function exactly(a: Line[], b: Line[]): Shown[] {
  if (a.length * b.length > EXACT) return abreast(a, b);

  const same = Array.from({ length: a.length + 1 }, () =>
    new Array<number>(b.length + 1).fill(0),
  );

  for (let i = a.length - 1; i >= 0; i--) {
    for (let j = b.length - 1; j >= 0; j--) {
      same[i][j] =
        a[i].text === b[j].text
          ? same[i + 1][j + 1] + 1
          : Math.max(same[i + 1][j], same[i][j + 1]);
    }
  }

  const shown: Shown[] = [];
  let i = 0;
  let j = 0;
  while (i < a.length && j < b.length) {
    if (a[i].text === b[j].text) {
      shown.push({ mark: " ", line: b[j] });
      i++;
      j++;
    } else if (same[i + 1][j] >= same[i][j + 1]) {
      shown.push({ mark: "−", line: a[i++] });
    } else {
      shown.push({ mark: "+", line: b[j++] });
    }
  }
  while (i < a.length) shown.push({ mark: "−", line: a[i++] });
  while (j < b.length) shown.push({ mark: "+", line: b[j++] });
  return shown;
}

/* How much of a file to show around a change.
 *
 * A definition can be a whole file, and a file can be ten thousand lines: a module holds
 * its own prose and every import, and a generated one holds all of it. Drawing that to
 * explain a change of four lines is slow to put on the page and slower to find anything
 * in. Far enough away, unchanged code stops being context and becomes the haystack. */
const REACH = 100;

/** What's worth showing: everything near a change, and a mark where the rest was. */
export function focused(lines: Shown[], reach = REACH): Shown[] {
  const changed = lines.flatMap((one, at) => (one.mark === " " ? [] : [at]));
  /* Nothing changed at all — a definition here because something it leans on moved — so
   * there's no change to sit near. The top of it is the part worth having. */
  const anchors = changed.length ? changed : [0];

  const near = new Set<number>();
  for (const at of anchors) {
    const [from, to] = [
      Math.max(0, at - reach),
      Math.min(lines.length - 1, at + reach),
    ];
    for (let line = from; line <= to; line++) near.add(line);
  }
  if (near.size === lines.length) return lines;

  const shown: Shown[] = [];
  let standing = false;
  for (let at = 0; at < lines.length; at++) {
    if (near.has(at)) {
      shown.push(lines[at]);
      standing = false;
    } else if (!standing) {
      shown.push({ mark: " ", line: { at: null, text: "…" } });
      standing = true;
    }
  }
  return shown;
}
