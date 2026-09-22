import type { Identity, Line, Occurrence, Review } from "./dagger.ts";

/* Turning one side of a definition into the lines a diff can be drawn from, and picking
 * out the names in a review that a highlighter is allowed to point at.
 */

/* One diff, not one per part. The split into parts decides what breaks callers; it isn't how
 * anybody reads code. Where a part's pieces aren't next to each other in the file — a
 * module's imports, an implementation's braces — a gap stands in rather than pretending the
 * lines met. */
export function stitch(occurrence: Occurrence | null): Line[] | null {
  if (!occurrence) return null;

  const pieces = Object.values(occurrence.parts)
    .flat()
    .sort((a, b) => a.span.start - b.span.start);

  const out: Line[] = [];
  let last: number | null = null;

  for (const piece of pieces) {
    /* Pieces that touch are run together exactly as the file has them. Putting a newline
     * between them instead is how a signature and its opening brace ended up on separate
     * lines. Only a real gap gets a line of its own — and that line is nowhere in the
     * file, so it has no number. */
    const joins = last !== null && piece.span.start === last;
    if (last !== null && !joins) out.push({ at: null, text: "…" });

    for (const [after, text] of piece.text.split("\n").entries()) {
      /* The first line of a piece carrying straight on from the last one finishes that
       * line rather than starting another. */
      if (joins && after === 0 && out.length) out[out.length - 1].text += text;
      else out.push({ at: piece.line + after, text });
    }
    last = piece.span.end;
  }

  return straighten(trimmed(out));
}

/** Blank lines at either end are the space around a definition, not part of it. */
function trimmed(lines: Line[]) {
  let from = 0;
  let until = lines.length;
  while (from < until && !lines[from].text.trim()) from += 1;
  while (until > from && !lines[until - 1].text.trim()) until -= 1;
  return lines.slice(from, until);
}

/* A definition starts at its name rather than at the margin, so its first line turns up
 * without the indentation every line beneath it still carries. Taking that much off the
 * rest lines them up the way the file has them. */
function straighten(lines: Line[]) {
  const under = lines.slice(1).filter((line) => line.text.trim() && line.at !== null);
  if (!under.length) return lines;

  const spare = Math.min(...under.map((line) => line.text.match(/^ */)![0].length));
  if (!spare) return lines;

  return lines.map((line, at) => (at ? { ...line, text: line.text.slice(spare) } : line));
}

/* Names in this review that can be pointed at without ambiguity.
 *
 * What a highlighter can't know: which words in this code are things the reader is about to
 * read, or has just read. A name appearing twice is left alone — pointing at the wrong one
 * is worse than pointing at nothing. */
export function namesIn(review: Review) {
  const seen = new Map<string, Identity | null>();
  for (const definition of review.definitions.values()) {
    seen.set(definition.name, seen.has(definition.name) ? null : definition.id);
  }
  const only = new Map<string, Identity>();
  for (const [name, id] of seen) if (id !== null) only.set(name, id);
  return only;
}
