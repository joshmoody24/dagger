import {
  createEffect,
  createUniqueId,
  For,
  type JSXElement,
  Show,
} from "solid-js";
import type { Review, Warning } from "./dagger.ts";
import "./Panels.css";

export type Opened = "warnings" | "cost" | null;

export function CostPanel(props: {
  open: boolean;
  onClose: () => void;
  review: Review;
}) {
  return (
    <Panel
      open={props.open}
      onClose={props.onClose}
      title="Cognitive load metrics"
    >
      <ul>
        <li>{props.review.cost.peak_open} definitions in mind at once</li>
        <li>{props.review.cost.taken_on_faith} definitions out of order</li>
        <li>
          {props.review.cost.jumps} {props.review.grouping || "file"} jumps
        </li>
        <li>{props.review.steps.length} definitions to read</li>
      </ul>
    </Panel>
  );
}

export function WarningsPanel(props: {
  open: boolean;
  onClose: () => void;
  hiding: Warning[];
  weaker: Warning[];
}) {
  return (
    <Panel
      open={props.open}
      onClose={props.onClose}
      title={
        props.hiding.length
          ? "This review might not be showing"
          : "Worked out a weaker way"
      }
    >
      <Show when={props.hiding.length}>
        <ul>
          <For each={props.hiding}>{(one) => <li>{one.message}</li>}</For>
        </ul>
      </Show>
      <Show when={props.weaker.length}>
        <Show when={props.hiding.length}>
          <h3>Worked out a weaker way</h3>
        </Show>
        <ul>
          <For each={props.weaker}>{(one) => <li>{one.message}</li>}</For>
        </ul>
      </Show>
    </Panel>
  );
}

/* Native <dialog> so focus trapping, Escape and the backdrop come for free. */
function Panel(props: {
  open: boolean;
  onClose: () => void;
  title: string;
  children: JSXElement;
}) {
  let box: HTMLDialogElement | undefined;
  const heading = createUniqueId();

  createEffect(() => {
    if (!box) return;
    if (props.open && !box.open) box.showModal();
    if (!props.open && box.open) box.close();
  });

  return (
    <dialog
      class="panel"
      ref={box}
      aria-labelledby={heading}
      onClose={() => props.onClose()}
      onClick={(event) => event.target === box && props.onClose()}
    >
      <h2 id={heading}>{props.title}</h2>
      {props.children}
    </dialog>
  );
}
