import { For, Show } from "solid-js";

/* What dagger is doing, while it does it.
 *
 * A review of a large repository takes a minute or two, almost all of it a compiler loading
 * a project, and a page saying nothing for that long is a page that looks broken. There's
 * no honest percentage to show — the slowest part, a language server indexing, reports
 * nothing until it's finished — so this shows the shape of the work instead: which stretch
 * is being done, which are finished, and how far through the one that can say.
 *
 * Everything here is read out of what dagger already tells whoever runs it. It says which
 * snapshot it's reading and how far through it is; this only arranges that on a page.
 */

interface Phase {
  said: string;
  detail?: string;
  through?: [number, number];
  done: boolean;
}

export function Waiting(props: { said: string[] }) {
  return (
    <div class="waiting">
      <ol class="phases">
        <For each={phases(props.said)}>
          {(phase, at) => (
            <li class={`phase ${standing(phases(props.said), at())}`}>
              <span class="what">{phase.said}</span>
              <Show when={standing(phases(props.said), at()) === "at" && phase.detail}>
                <span class="detail">{phase.detail}</span>
              </Show>
              <Show when={standing(phases(props.said), at()) === "at" && phase.through}>
                <span class="through">
                  <i style={{ width: `${(phase.through![0] / Math.max(phase.through![1], 1)) * 100}%` }} />
                </span>
              </Show>
            </li>
          )}
        </For>
      </ol>
    </div>
  );
}

/* Done, being done, or not started. Only the one being done moves: a row of circles all
 * beating at once reads as several things happening together, and only one is. */
function standing(phases: Phase[], at: number) {
  if (phases[at].done) return "was";
  return phases.findIndex((phase) => !phase.done) === at ? "at" : "yet";
}

/* The whole shape up front, filled in as it happens.
 *
 * There are always three stretches — laying both snapshots out, then reading each of them —
 * so all three are shown from the start and the reader can see how much is left rather than
 * watching steps appear one at a time with no idea how many are coming. Which revisions
 * they are comes from the first thing dagger says.
 */
function phases(said: string[]): Phase[] {
  const pair = said
    .map((line) => line.match(/^comparing (\S+) to (\S+)/))
    .find(Boolean);
  const [before, after] = pair ? [pair[1], pair[2]] : ["the first snapshot", "the second"];

  const found: Phase[] = [
    { said: "laying out both snapshots", done: false },
    { said: `reading ${before}`, done: false },
    { said: `reading ${after}`, done: false },
  ];

  let at = 0;
  for (const line of said) {
    if (!line.trim()) continue;

    const reading = line.match(/^reading \d+ files of (\S+) with (\S+)/);
    if (reading) {
      at = reading[1] === before ? 1 : 2;
      found[at].detail = `with ${reading[2].split("/").pop()}`;
      found[at].through = undefined;
      continue;
    }

    /* Indented lines belong to whatever is being read now. */
    if (line.startsWith(" ")) {
      found[at].detail = line.trim();
      const through = line.match(/(\d+) of (\d+)/);
      found[at].through = through ? [Number(through[1]), Number(through[2])] : undefined;
      continue;
    }

    if (at === 0) found[0].detail = line.trim();
  }

  for (const [index, phase] of found.entries()) phase.done = index < at;
  return found;
}
