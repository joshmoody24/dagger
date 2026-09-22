/* What dagger says while it works, read as the shape of the work.
 *
 * Plain functions, no components: this is the part worth testing without a browser, and the
 * part most likely to go quietly wrong — it reads lines written by another program, and a
 * misread line looks exactly like a slow one.
 */

export interface Phase {
  said: string;
  detail?: string | undefined;
  through?: [number, number] | undefined;
  done: boolean;
  going: boolean;
}

/* Done, being done, or not started. Taken from what each phase has said for itself rather
 * than from its place in the row, so a phase is under way exactly when there's something
 * saying so. */
export function standing(phases: Phase[], at: number) {
  return phases[at].done ? "was" : phases[at].going ? "at" : "yet";
}

/* The whole shape up front, filled in as it happens.
 *
 * There are always three stretches — laying both snapshots out, then reading each of them —
 * so all three are shown from the start and the reader can see how much is left rather than
 * watching steps appear one at a time with no idea how many are coming. Which revisions
 * they are comes from the first thing dagger says.
 *
 * Whose a line is comes from the line itself — dagger puts the side in front of everything
 * it says — rather than from what was said before it. Worked out from the order instead,
 * this reads any change to how the two are run as a change to what the reader is told.
 */
export function phases(said: string[]): Phase[] {
  const pair = said
    .map((line) => line.match(/^comparing (\S+) to (\S+)/))
    .find(Boolean);
  const [before, after] = pair
    ? [pair[1], pair[2]]
    : ["the first snapshot", "the second"];

  const found: Phase[] = [
    { said: "laying out both snapshots", done: false, going: true },
    { said: `reading ${before}`, done: false, going: false },
    { said: `reading ${after}`, done: false, going: false },
  ];

  for (const line of said) {
    if (!line.trim()) continue;

    /* "before · read 12 of 40 files", and anything without a side belongs to the laying
     * out, which happens before either reading starts. */
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
