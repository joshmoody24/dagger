import {
  createEffect,
  createMemo,
  createSignal,
  on,
  onCleanup,
  onMount,
} from "solid-js";
import type { Identity, Laid, Review } from "./dagger.ts";
import { every, hit } from "./graph/hit.ts";
import { paint, palette } from "./graph/paint.ts";
import { CLOSEST, viewport } from "./graph/viewport.ts";
import { editing } from "./keys.ts";
import { SpokenGraph } from "./SpokenGraph.tsx";
import { wearing } from "./theme.ts";
import "./Graph.css";

/* Drawn on a canvas. SpokenGraph keeps it keyboard/screen-reader accessible. */

interface GraphProps {
  review: Review;
  laid: Laid;
  here: Identity | null;
  next: Identity | null;
  read: Set<Identity>;
  onOpen: (id: Identity) => void;
}

export function Graph(props: GraphProps) {
  let frame!: HTMLDivElement;
  let paper!: HTMLCanvasElement;
  let ink!: CanvasRenderingContext2D;

  const [over, setOver] = createSignal<string | null>(null);
  const [touching, setTouching] = createSignal<Identity | null>(null);

  const view = viewport({
    paper: () => paper,
    room: () => frame.getBoundingClientRect(),
    laid: () => props.laid,
    onZoom: () => redraw(),
  });

  const spotOf = (id: Identity | null) =>
    id === null ? null : (props.laid.at.get(id) ?? null);

  const closer = () => {
    const spot = spotOf(props.here);
    if (spot) view.onto(spot, CLOSEST / 2);
  };

  /* ---------------- what is where ---------------- */

  const boxes = createMemo(() => every(props.laid.boxes));

  const touched = (event: { clientX: number; clientY: number }) => {
    const what = hit(props.laid, boxes(), view.pointing(event));
    if (!what) return null;
    if ("box" in what) return { file: what.box.key };
    return {
      node: what.node,
      file: props.review.definitions.get(what.node)?.file ?? "",
    };
  };

  const onPointerMove = (event: PointerEvent) => {
    const what = touched(event);
    setOver(what ? what.file : null);
    setTouching(what && "node" in what ? what.node : null);
  };
  const hint = () => {
    const id = touching();
    return id === null ? "" : (props.review.definitions.get(id)?.path ?? "");
  };

  const onClick = (event: MouseEvent) => {
    const what = touched(event);
    if (what && "node" in what) props.onOpen(what.node);
  };

  /* ---------------- drawing ---------------- */

  /* getComputedStyle forces a style flush, so only re-read the palette on theme change. */
  const tint = createMemo(() => {
    void wearing();
    return palette(getComputedStyle(document.documentElement));
  });

  const scene = () => ({
    review: props.review,
    laid: props.laid,
    here: props.here,
    next: props.next,
    read: props.read,
    over: over(),
    touching: touching(),
    view: view.seen(),
    room: frame.getBoundingClientRect(),
    dense: window.devicePixelRatio || 1,
  });

  const draw = () => paint(ink, scene(), tint());

  let drawing = 0;
  const redraw = () => {
    if (drawing || !ink) return;
    drawing = requestAnimationFrame(() => {
      drawing = 0;
      draw();
    });
  };

  /* Backing store scaled by devicePixelRatio, or text is blurry on dense screens. */
  const sized = () => {
    const room = frame.getBoundingClientRect();
    const dense = window.devicePixelRatio || 1;
    paper.width = Math.round(room.width * dense);
    paper.height = Math.round(room.height * dense);
    paper.style.width = `${room.width}px`;
    paper.style.height = `${room.height}px`;
  };

  /* ---------------- keeping up ---------------- */

  const onKey = (event: KeyboardEvent) => {
    if (event.metaKey || event.altKey || editing(event.target)) return;
    if (event.key === "+" || event.key === "=") view.scaleBy(1.2);
    else if (event.key === "-" || event.key === "_") view.scaleBy(1 / 1.2);
    else if (event.key === "0") view.fit();
    else if (event.key === "1") closer();
    else return;
    event.preventDefault();
  };

  onMount(() => {
    ink = paper.getContext("2d")!;
    sized();
    view.attach();
    frame.addEventListener("wheel", view.onWheel, { passive: true });

    /* draw() directly, not redraw(): sizing clears the canvas, and rAF callbacks run
     * before ResizeObserver callbacks, so a deferred redraw would leave a blank frame
     * (visible as flicker while dragging the sidebar). */
    const resized = new ResizeObserver(() => {
      sized();
      view.bounded();
      if (view.seen().k < view.whole().k) view.fit();
      draw();
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

  /* Everything the scene is drawn from, so a change to any of it redraws. */
  createEffect(
    on(
      () => [
        props.here,
        props.next,
        props.read,
        props.review,
        props.laid,
        over(),
        touching(),
        tint(),
      ],
      redraw,
    ),
  );

  /* Pan to the current definition only when it's off screen. */
  createEffect(() => {
    const spot = spotOf(props.here);
    if (!spot || !paper) return;
    if (!view.shows(spot)) view.onto(spot, view.seen().k);
  });

  return (
    <div
      class="canvas"
      ref={frame}
      style={{ cursor: over() === null ? "grab" : "pointer" }}
      title={hint()}
      onPointerMove={onPointerMove}
      onPointerLeave={() => {
        setOver(null);
        setTouching(null);
      }}
      onClick={onClick}
    >
      <canvas
        ref={paper}
        role="img"
        aria-label="The definitions this change touches, and what holds up what"
      />

      <SpokenGraph review={props.review} onOpen={props.onOpen} />

      <div class="viewkeys">
        <kbd>0</kbd> fit all · <kbd>1</kbd> zoom to current · <kbd>t</kbd>{" "}
        {wearing()}
      </div>
    </div>
  );
}
