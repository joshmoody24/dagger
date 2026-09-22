import {
  createEffect,
  createMemo,
  createSignal,
  For,
  on,
  onCleanup,
  onMount,
  Show,
} from "solid-js";
import type { Identity, Laid, Review } from "./dagger.ts";
import { scene, BOX_TEXT_Y, NODE_TEXT_Y } from "./graph/scene.ts";
import { CLOSEST, viewport } from "./graph/viewport.ts";
import { editing } from "./keys.ts";
import { RADIUS } from "./layout.ts";
import { wearing } from "./theme.ts";
import "./Graph.css";

interface GraphProps {
  review: Review;
  laid: Laid;
  here: Identity | null;
  next: Identity | null;
  read: Set<Identity>;
  query: string;
  onOpen: (id: Identity) => void;
}

export function Graph(props: GraphProps) {
  let frame!: HTMLDivElement;
  let paper!: SVGSVGElement;

  const [over, setOver] = createSignal<string | null>(null);
  const [touching, setTouching] = createSignal<Identity | null>(null);
  const [seen, setSeen] = createSignal({ k: 1, x: 0, y: 0 });

  const view = viewport({
    paper: () => paper,
    room: () => frame.getBoundingClientRect(),
    laid: () => props.laid,
    onZoom: () => setSeen(view.seen()),
  });

  const spotOf = (id: Identity | null) =>
    id === null ? null : (props.laid.at.get(id) ?? null);

  const closer = () => {
    const spot = spotOf(props.here);
    if (spot) view.onto(spot, CLOSEST / 2);
  };

  const shown = createMemo(() =>
    scene({
      review: props.review,
      laid: props.laid,
      here: props.here,
      next: props.next,
      read: props.read,
      over: over(),
      touching: touching(),
      query: props.query,
    }),
  );

  const onKey = (event: KeyboardEvent) => {
    if (event.metaKey || event.altKey || editing(event.target)) return;
    if (event.key === "+") view.scaleBy(1.2);
    else if (event.key === "-" || event.key === "_") view.scaleBy(1 / 1.2);
    else if (event.key === "0") view.fit();
    else if (event.key === "=") closer();
    else return;
    event.preventDefault();
  };

  onMount(() => {
    view.attach();
    frame.addEventListener("wheel", view.onWheel, { passive: true });

    const resized = new ResizeObserver(() => {
      view.bounded();
      if (view.seen().k < view.whole().k) view.fit();
    });
    resized.observe(frame);

    /* A new layout gets fresh extents and a fit-all view. */
    createEffect(on(() => props.laid, view.fit));

    document.addEventListener("keydown", onKey);
    onCleanup(() => {
      resized.disconnect();
      frame.removeEventListener("wheel", view.onWheel);
      document.removeEventListener("keydown", onKey);
    });
  });

  /* Pan to the current definition only when it's off screen. */
  createEffect(() => {
    const spot = spotOf(props.here);
    if (!spot || !paper) return;
    if (!view.shows(spot)) view.onto(spot, view.seen().k);
  });

  return (
    <div class="graph" ref={frame}>
      <svg
        ref={paper}
        role="img"
        aria-label="The definitions this change touches, and what depends on what"
      >
        <g transform={`translate(${seen().x} ${seen().y}) scale(${seen().k})`}>
          <For each={shown().groups}>
            {(group) => (
              <g
                class="group"
                classList={{ nested: group.depth > 0, lit: group.lit }}
              >
                <rect
                  x={group.x}
                  y={group.y}
                  width={group.w}
                  height={group.h}
                  rx={group.radius}
                  onPointerEnter={() => setOver(group.key)}
                  onPointerLeave={() => setOver(null)}
                />
                <text x={group.x + 10} y={group.y + BOX_TEXT_Y}>
                  {group.label}
                </text>
              </g>
            )}
          </For>
          <For each={shown().edges}>
            {(edge) => (
              <path
                class={`edge ${edge.kind}`}
                classList={{ faith: edge.faith }}
                d={edge.path}
              />
            )}
          </For>
          <Show when={shown().ahead}>
            {(ahead) => (
              <g class="ahead">
                <path class="shaft" d={ahead().path} />
                <path class="tip" d={ahead().tip} />
              </g>
            )}
          </Show>
          <For each={shown().definitions}>
            {(definition) => (
              <g
                class={[
                  "definition",
                  definition.tint,
                  ...definition.classes,
                ].join(" ")}
                tabindex="0"
                onPointerEnter={() => setTouching(definition.id)}
                onPointerLeave={() => setTouching(null)}
                onClick={() => props.onOpen(definition.id)}
                onKeyDown={(event) => {
                  if (event.key === "Enter") props.onOpen(definition.id);
                }}
              >
                <title>{definition.title}</title>
                <rect
                  x={definition.x}
                  y={definition.y}
                  width={definition.w}
                  height={definition.h}
                  rx={RADIUS.definition}
                />
                <text
                  class="mark"
                  x={definition.x + 10}
                  y={definition.y + NODE_TEXT_Y}
                >
                  {definition.mark}
                </text>
                <text
                  class="name"
                  x={definition.x + definition.nameX}
                  y={definition.y + NODE_TEXT_Y}
                >
                  {definition.name}
                </text>
              </g>
            )}
          </For>
        </g>
      </svg>

      <div class="viewkeys">
        <kbd>0</kbd> fit all · <kbd>=</kbd> zoom to current · <kbd>t</kbd>{" "}
        {wearing()}
      </div>
    </div>
  );
}
