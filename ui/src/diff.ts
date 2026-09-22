import type { Line, Shown } from "./dagger.ts";

/* Line-by-line diff of two versions of a definition. Kept separate because it's the one
 * part where speed matters: a definition can be a 6000-line file.
 */

/* Largest a×b table worth building; 6000×6000 is not. */
const EXACT = 40_000;

/** Character range within a line, end exclusive. */
export type Range = [number, number];

/** A diff line with what the page adds: words to stress, or the stretch it stands in for. */
export interface Detailed extends Shown {
  emphasis?: Range[];
  /** Indexes into the full diff of the lines elided here. */
  gap?: { from: number; to: number };
}

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
  const once = (lines: Line[]) =>
    new Map(
      [...Map.groupBy(lines.entries(), ([, line]) => line.text)].flatMap(
        ([text, found]): [string, number][] =>
          found.length === 1 ? [[text, found[0][0]]] : [],
      ),
    );
  const [inA, inB] = [once(a), once(b)];

  const pairs = [...inA].flatMap(([text, at]): [number, number][] => {
    const there = inB.get(text);
    return there === undefined ? [] : [[at, there]];
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
  return [
    ...shown,
    ...a.slice(i).map((line) => ({ mark: "−" as const, line })),
    ...b.slice(j).map((line) => ({ mark: "+" as const, line })),
  ];
}

/* A removed line followed by an added one is usually an edit, not a swap; a run of n and n
 * likewise, in order. Shorter than this in common and they're just different lines. */
const ALIKE = 0.5;

/** Removed-then-added runs paired in order, each pair stressing the words that differ. */
export function paired(lines: Shown[]): Detailed[] {
  const run = (from: number, mark: Shown["mark"]) => {
    let to = from;
    while (to < lines.length && lines[to].mark === mark) to++;
    return to - from;
  };

  const stressed = new Map<number, Range[]>();
  for (let at = 0; at < lines.length;) {
    const gone = run(at, "−");
    const came = gone ? run(at + gone, "+") : 0;
    for (let k = 0; gone === came && k < gone; k++) {
      const [was, is] = [at + k, at + gone + k];
      const differ = differing(lines[was].line.text, lines[is].line.text);
      if (!differ) continue;
      stressed.set(was, differ[0]);
      stressed.set(is, differ[1]);
    }
    at += gone + came || 1;
  }

  return lines.map((one, at) => {
    const emphasis = stressed.get(at);
    return emphasis ? { ...one, emphasis } : one;
  });
}

interface Token {
  text: string;
  from: number;
}

/* Words, runs of whitespace, and single punctuation marks, each knowing where it starts. */
function tokens(text: string): Token[] {
  return [...text.matchAll(/\w+|\s+|./g)].map((hit) => ({
    text: hit[0],
    from: hit.index,
  }));
}

/* Where two lines differ, as ranges on each, or null when they share too little for the
 * differences to mean anything. */
function differing(a: string, b: string): [Range[], Range[]] | null {
  const [x, y] = [tokens(a), tokens(b)];
  if (x.length * y.length > EXACT) return null;

  const same = Array.from({ length: x.length + 1 }, () =>
    new Array<number>(y.length + 1).fill(0),
  );
  for (let i = x.length - 1; i >= 0; i--) {
    for (let j = y.length - 1; j >= 0; j--) {
      same[i][j] =
        x[i].text === y[j].text
          ? same[i + 1][j + 1] + 1
          : Math.max(same[i + 1][j], same[i][j + 1]);
    }
  }
  if (same[0][0] < ALIKE * Math.max(x.length, y.length)) return null;

  const [keptX, keptY] = [new Set<number>(), new Set<number>()];
  for (let i = 0, j = 0; i < x.length && j < y.length;) {
    if (x[i].text === y[j].text) {
      keptX.add(i);
      keptY.add(j);
      i++;
      j++;
    } else if (same[i + 1][j] >= same[i][j + 1]) i++;
    else j++;
  }
  return [ranges(x, keptX), ranges(y, keptY)];
}

/* The tokens not kept, as ranges, with neighbours joined into one. */
function ranges(all: Token[], kept: Set<number>): Range[] {
  return all.reduce<Range[]>((out, token, at) => {
    if (kept.has(at)) return out;
    const [from, to] = [token.from, token.from + token.text.length];
    const last = out.at(-1);
    return last && last[1] === from
      ? [...out.slice(0, -1), [last[0], to]]
      : [...out, [from, to]];
  }, []);
}

/* Lines of context around a change. A definition can be a 10,000-line file, and far from a
 * change unchanged code is just noise. */
const REACH = 100;

/** What's worth showing: everything near a change, and a gap recording the rest. */
export function focused(lines: Detailed[], reach = REACH): Detailed[] {
  const changed = lines.flatMap((one, at) => (one.mark === " " ? [] : [at]));

  /* The first line is the signature, which says what's being read, so it always shows. */
  const near = new Set<number>([0]);
  for (const at of changed) {
    const [from, to] = [
      Math.max(0, at - reach),
      Math.min(lines.length - 1, at + reach),
    ];
    for (let line = from; line <= to; line++) near.add(line);
  }
  /* Nothing changed (it's here because a dependency changed), so show the top. */
  if (!changed.length) for (let at = 0; at <= reach; at++) near.add(at);
  if (near.size >= lines.length) return lines;

  /* Each stretch stood down, keyed by where it starts. */
  const gaps = new Map<number, number>();
  for (let at = 0, from = 0; at < lines.length; at++) {
    if (near.has(at)) continue;
    if (!gaps.has(from) || gaps.get(from) !== at) from = at;
    gaps.set(from, at + 1);
  }

  return lines.flatMap((one, at): Detailed[] => {
    const to = gaps.get(at);
    if (near.has(at)) return [one];
    if (to === undefined) return [];
    return [
      { mark: " ", line: { at: null, text: "…" }, gap: { from: at, to } },
    ];
  });
}
