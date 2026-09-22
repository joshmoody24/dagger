/* Reads dagger's progress lines into phases. Plain functions so it can be tested without a
 * browser: a misread line looks exactly like a slow one.
 */

export interface Phase {
  said: string;
  detail?: string | undefined;
  through?: [number, number] | undefined;
  done: boolean;
  going: boolean;
}

/* Taken from what each phase has said rather than from its position in the row. */
export function standing(phases: Phase[], at: number) {
  return phases[at].done ? "was" : phases[at].going ? "at" : "yet";
}

/* All three phases are shown from the start so the reader can see how much is left. Which
 * side a line belongs to comes from the line itself, never from what came before it, since
 * the two readings interleave. */
/** The two things being compared, as the snapshot adapter named them. */
export function compared(said: string[]): [string, string] | null {
  const pair = said
    .map((line) => line.match(/^comparing (\S+) to (\S+)/))
    .find(Boolean);
  return pair ? [pair[1], pair[2]] : null;
}

export function phases(said: string[]): Phase[] {
  const [before, after] = compared(said) ?? [
    "the first snapshot",
    "the second snapshot",
  ];

  const found: Phase[] = [
    { said: "laying out both snapshots", done: false, going: true },
    { said: `reading ${before}`, done: false, going: false },
    { said: `reading ${after}`, done: false, going: false },
  ];

  for (const line of said) {
    if (!line.trim()) continue;

    /* A line with no side belongs to laying out, which happens before either reading. */
    const [side, rest] = told(line);
    if (!side) {
      found[0].detail = line.trim();
      continue;
    }

    const phase = found[side === "before" ? 1 : 2];
    phase.going = true;
    found[0].done = true;

    const reading = rest.match(/^reading \d+ files of \S+ with (\S+)/);
    if (reading) {
      phase.detail = `with ${reading[1].split("/").pop()}`;
      phase.through = undefined;
      // A side with several extractors reports "read" once per extractor; it's done
      // only when the last one has.
      phase.done = false;
      continue;
    }

    const finished = rest.match(/^read \d+ files, found (\d+) definitions/);
    if (finished) {
      phase.detail = `${finished[1]} definitions`;
      phase.through = undefined;
      phase.done = true;
      continue;
    }

    phase.detail = rest;
    const through = rest.match(/(\d+) of (\d+)/);
    phase.through = through
      ? [Number(through[1]), Number(through[2])]
      : undefined;
  }

  return found;
}

/** Which snapshot a line is about, and what it says. */
function told(line: string): [string | null, string] {
  const at = line.indexOf(" · ");
  if (at < 0) return [null, line.trim()];
  return [line.slice(0, at).trim(), line.slice(at + 3).trim()];
}
