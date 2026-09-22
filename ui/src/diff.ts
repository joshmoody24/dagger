import type { Line, Shown } from "./dagger.ts";

/* Line-by-line diff of two versions of a definition. Kept separate because it's the one
 * part where speed matters: a definition can be a 6000-line file.
 */

/* Largest a×b table worth building; 6000×6000 is not. */
const EXACT = 40_000;

/** Line by line, marked as kept, gone, or new. */
export function compare(before: Line[] | null, after: Line[] | null): Shown[] {
  return diffing(before ?? [], after ?? []);
}

/* The exact LCS table is quadratic, so the common head and tail are stripped first and the
 * middle is split at lines unique to both sides. What's left is small enough for the table.
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

/* A line appearing exactly once on each side must match itself, so those lines split one
 * big comparison into many small ones.
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

/* Position-by-position fallback for a stretch too big for the table with no unique line to
 * anchor on. Not the shortest diff, but a correct one in a single pass.
 */
function abreast(a: Line[], b: Line[]): Shown[] {
  const shown: Shown[] = [];
  for (let at = 0; at < Math.max(a.length, b.length); at++) {
    const [was, is] = [a[at], b[at]];
    if (was && is && was.text === is.text) {
      shown.push({ mark: " ", line: is });
      continue;
    }
    /* Removals before additions, so filtering out either mark rebuilds the other side. */
    if (was) shown.push({ mark: "−", line: was });
    if (is) shown.push({ mark: "+", line: is });
  }
  return shown;
}

/* Longest increasing subsequence: an anchor that goes backwards would make the diff cross
 * over itself.
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

/* Guarded here rather than at each caller. A stretch with no line unique to both sides (a
 * file of repeated punctuation, say) can't be split, so it can still arrive too big. */
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

/* Lines of context around a change. A definition can be a 10,000-line file, and far from a
 * change unchanged code is just noise. */
const REACH = 100;

/** What's worth showing: everything near a change, and a mark where the rest was. */
export function focused(lines: Shown[], reach = REACH): Shown[] {
  const changed = lines.flatMap((one, at) => (one.mark === " " ? [] : [at]));
  /* Nothing changed (it's here because a dependency changed), so show the top. */
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
