import { createMemo, For, Show } from "solid-js";
import { phases, standing } from "./progress.ts";

/* Shows phases rather than a percentage: the slowest phase (language server indexing)
 * reports nothing until it's done, so no honest overall progress exists. */

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
                  <i
                    style={{
                      width: `${(phase.through![0] / Math.max(phase.through![1], 1)) * 100}%`,
                    }}
                  />
                </span>
              </Show>
            </li>
          )}
        </For>
      </ol>
    </div>
  );
}
