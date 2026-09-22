import { createSignal, For, onCleanup, Show } from "solid-js";
import type { Definition as Def, Identity, Review, Step } from "./dagger.ts";
import { MARK, TINT, broke } from "./digest.ts";
import { Diff, type Names, STEP } from "./Diff.tsx";
import { useHeldKeys } from "./keys.ts";
import { ReadingNav } from "./ReadingNav.tsx";
import { Resizer } from "./Resizer.tsx";
import "./Reading.css";

/** How far the drawer is open on a narrow screen. */
export type Sheet = "closed" | "half" | "full";
/** Where the sheet sits: a column on a wide screen, a drawer on a narrow one. */
export type Facing = "beside" | "away" | Sheet;

/* j/k scroll speed in px/s. About sixty lines: fast enough to cross a long definition,
 * slow enough to still read on the way past. */
const SPEED = 1240;

interface ReadingProps {
  review: Review;
  here: Identity | null;
  step: Step | undefined;
  at: number;
  names: Names;
  read: boolean;
  sheet: Facing;
  onStep: (by: number) => void;
  onRead: (by: number) => void;
  onOpen: (id: Identity) => void;
  onToggle: () => void;
  onExpand: () => void;
  onClose: () => void;
}

export function Reading(props: ReadingProps) {
  const definition = () =>
    props.here === null ? undefined : props.review.definitions.get(props.here);
  const leans = () =>
    props.review.edges
      .filter((edge) => edge.from === definition()?.id)
      .map((edge) => edge.to);

  const because = () => leans().filter((id) => broke(props.review, id));
  const uses = () => leans().filter((id) => !broke(props.review, id));

  /* Dragged width of the column; 0 leaves the stylesheet's default. */
  const [width, setWidth] = createSignal(0);

  let body: HTMLDivElement | undefined;

  /* j/k scroll via rAF between keydown and keyup rather than per key repeat, because
   * repeat timing (delay, then bursts) makes the scroll stutter. */
  let going = 0;
  let rolling = 0;
  let last = 0;

  const roll = (now: number) => {
    if (!body || !going) {
      rolling = 0;
      return;
    }
    // Time-based so speed is the same at any refresh rate.
    const since = last ? Math.min(now - last, 100) : 16;
    last = now;
    body.scrollTop += going * SPEED * (since / 1000);
    rolling = requestAnimationFrame(roll);
  };

  useHeldKeys(
    { j: 1, k: -1 },
    (way) => {
      going = way;
      last = 0;
      if (!rolling) rolling = requestAnimationFrame(roll);
    },
    (way) => {
      if (going === way) going = 0;
    },
  );
  onCleanup(() => rolling && cancelAnimationFrame(rolling));

  return (
    <Show when={definition()}>
      {(one) => (
        <aside
          class={`sheet ${props.sheet}`}
          style={
            props.sheet === "beside" && width()
              ? { width: `${width()}px` }
              : undefined
          }
        >
          <Show when={props.sheet === "beside"}>
            <Resizer onResize={setWidth} />
          </Show>
          <div class="sheet-head">
            <b class={`mark ${TINT[one().mark]}`}>{MARK[one().mark]}</b>
            <h2>{one().path}</h2>
            <button
              class="icon-button grip"
              onClick={() => props.onExpand()}
              aria-label={props.sheet === "full" ? "Shrink" : "Expand"}
            >
              {props.sheet === "full" ? "⌄" : "⌃"}
            </button>
            <button
              class="icon-button grip"
              onClick={() => props.onClose()}
              aria-label="Close"
            >
              ×
            </button>
          </div>

          <div class="sheet-meta">
            <About
              definition={one()}
              because={because()}
              uses={uses()}
              step={props.step}
              review={props.review}
              onOpen={props.onOpen}
            />
            <p class="legend">
              Names in the code open their definitions, coloured by how each
              changed. A collapsed run opens {STEP} lines at a click or with o.
            </p>
          </div>

          <div class="sheet-body" ref={body}>
            <Diff
              definition={one()}
              names={props.names}
              review={props.review}
              onOpen={props.onOpen}
            />
          </div>

          <ReadingNav
            at={props.at}
            total={props.review.steps.length}
            read={props.read}
            onStep={props.onStep}
            onRead={props.onRead}
            onToggle={props.onToggle}
          />
        </aside>
      )}
    </Show>
  );
}

function About(props: {
  definition: Def;
  because: Identity[];
  uses: Identity[];
  review: Review;
  onOpen: (id: Identity) => void;
  step: Step | undefined;
}) {
  return (
    <>
      <p class="file">
        {props.definition.file}
        <span class="kind">{props.definition.kind}</span>
      </p>

      <Show when={props.because.length}>
        <p class="why">
          because:{" "}
          <Names
            ids={props.because}
            step={props.step}
            review={props.review}
            onOpen={props.onOpen}
          />
        </p>
      </Show>
      <Show when={props.uses.length}>
        <p class="why">
          uses:{" "}
          <Names
            ids={props.uses}
            step={props.step}
            review={props.review}
            onOpen={props.onOpen}
          />
        </p>
      </Show>
    </>
  );
}

function Names(props: {
  ids: Identity[];
  review: Review;
  step: Step | undefined;
  onOpen: (id: Identity) => void;
}) {
  const shown = () =>
    props.ids
      .map((id) => props.review.definitions.get(id))
      .filter((one): one is Def => one !== undefined)
      .map((definition) => ({
        definition,
        /* No step (viewing out of order) means nothing can be "not yet seen". */
        soon: (props.step?.on_faith || []).includes(definition.id),
      }));

  return (
    <For each={shown()}>
      {(one, index) => (
        <>
          <Show when={index() > 0}>, </Show>
          <button
            type="button"
            class={one.soon ? "soon" : ""}
            onClick={() => props.onOpen(one.definition.id)}
          >
            {one.definition.name}
            {one.soon ? " (not yet seen)" : ""}
          </button>
        </>
      )}
    </For>
  );
}
