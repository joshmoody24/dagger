import type { Identity, Line, Occurrence, Review } from "./dagger.ts";

/* One side of a definition as diffable lines, and the names a highlighter may link to. */

/* One diff, not one per part: parts are about what breaks callers, not how anyone reads
 * code. Pieces that aren't adjacent in the file get a gap line between them. */
export function stitch(occurrence: Occurrence | null): Line[] | null {
  if (!occurrence) return null;

  const pieces = Object.values(occurrence.parts)
    .flat()
    .sort((a, b) => a.span.start - b.span.start);

  const out: Line[] = [];
  let last: number | null = null;

  for (const piece of pieces) {
    /* Touching pieces are joined as the file has them (a signature and its opening brace,
     * say). Only a real gap gets a gap line, which is not in the file so has no number. */
    const joins = last !== null && piece.span.start === last;
    if (last !== null && !joins) out.push({ at: null, text: "…" });

    for (const [after, text] of piece.text.split("\n").entries()) {
      if (joins && after === 0 && out.length) out[out.length - 1].text += text;
      else out.push({ at: piece.line + after, text });
    }
    last = piece.span.end;
  }

  return straighten(trimmed(out));
}

/** Blank lines at either end are the space around a definition, not part of it. */
function trimmed(lines: Line[]) {
  const written = (line: Line) => Boolean(line.text.trim());
  const from = lines.findIndex(written);
  return from < 0 ? [] : lines.slice(from, lines.findLastIndex(written) + 1);
}

/* The first line starts at the name, not the margin, so it lacks the indentation the lines
 * below still carry. Strip that much from the rest. */
function straighten(lines: Line[]) {
  const under = lines
    .slice(1)
    .filter((line) => line.text.trim() && line.at !== null);
  if (!under.length) return lines;

  const spare = Math.min(
    ...under.map((line) => line.text.match(/^ */)![0].length),
  );
  if (!spare) return lines;

  return lines.map((line, at) =>
    at ? { ...line, text: line.text.slice(spare) } : line,
  );
}

/* Names that appear exactly once, so the highlighter can link them. Linking the wrong one
 * is worse than linking nothing. */
export function namesIn(review: Review): Map<string, Identity> {
  const named = Map.groupBy(review.definitions.values(), (one) => one.name);
  return new Map(
    [...named].flatMap(([name, ones]): [string, Identity][] =>
      ones.length === 1 ? [[name, ones[0].id]] : [],
    ),
  );
}
