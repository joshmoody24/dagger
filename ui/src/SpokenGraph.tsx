import { For, Show } from "solid-js";
import type { Identity, Review } from "./dagger.ts";

/* Keyboard and screen-reader access to the graph, since the canvas has none. */
export function SpokenGraph(props: {
  review: Review;
  onOpen: (id: Identity) => void;
}) {
  return (
    <ul class="spoken" aria-label="What changed, and what holds up what">
      <For each={props.review.steps}>
        {(step) => (
          <Show when={props.review.definitions.get(step.definition)}>
            {(definition) => (
              <li>
                <button onClick={() => props.onOpen(definition().id)}>
                  {definition().path} — {definition().kind}
                </button>
              </li>
            )}
          </Show>
        )}
      </For>
    </ul>
  );
}
