import { createMemo, For, Show } from "solid-js";
import { phases, standing } from "./progress.ts";

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

export function Waiting(props: { said: string[] }) {
  const shape = createMemo(() => phases(props.said));
  return (
    <div class="waiting">
      <ol class="phases">
        <For each={shape()}>
          {(phase, at) => (
            <li class={`phase ${standing(shape(), at())}`}>
              <span class="what">{phase.said}</span>
              <Show when={standing(shape(), at()) === "at" && phase.detail}>
                <span class="detail">{phase.detail}</span>
              </Show>
              <Show when={standing(shape(), at()) === "at" && phase.through}>
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
