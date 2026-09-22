import {
  createEffect,
  createMemo,
  createSignal,
  For,
  onCleanup,
  onMount,
  Show,
  untrack,
} from "solid-js";
import { select } from "d3-selection";
import { zoom as zooming, zoomIdentity, zoomTransform } from "d3-zoom";
import type { Box, Edge, Identity, Laid, Review, Spot } from "./dagger.ts";
import { MARK, TINT } from "./digest.ts";
import { BOX_FONT, FONT, RADIUS, shorten } from "./layout.ts";
import { wearing } from "./theme.ts";

const EDGE = 24;
/* Max zoom. Past this the text is too big to be useful. */
const CLOSEST = 4;
const MONO = "ui-monospace, SFMono-Regular, Menlo, Consolas, monospace";

/* Drawn on a canvas rather than DOM elements: hundreds of SVG nodes ran at ~10fps in the
 * Tauri webview versus 60fps here. The hidden list below keeps it keyboard/screen-reader
 * accessible. The d3 zoom behaviour is the single owner of the view transform; nothing
 * sets it directly. */
interface Point {
  x: number;
  y: number;
}

type Touched = { node: Identity; file: string } | { box: Box; file: string };

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

  const behaviour = zooming<HTMLCanvasElement, unknown>()
    /* Wheel is handled by onWheel instead. */
    .filter(
      (event: Event & { ctrlKey?: boolean; button?: number }) =>
        event.type !== "wheel" && !event.ctrlKey && !event.button,
    )
    .on("zoom", () => redraw());

  const pane = () => frame.getBoundingClientRect();
  const seen = () => zoomTransform(paper);

  /* ---------------- where the view is ---------------- */

  /* Fit-all transform. Also the minimum zoom. */
  const whole = () => {
    const room = pane();
    const k = Math.min(
      (room.width - 2 * EDGE) / props.laid.w,
      (room.height - 2 * EDGE) / props.laid.h,
    );
    return zoomIdentity
      .translate(
        (room.width - props.laid.w * k) / 2,
        (room.height - props.laid.h * k) / 2,
      )
      .scale(k);
  };

  /* Re-run whenever the layout changes size (e.g. ripples toggled), or the old extents
   * keep constraining the view. */
  const bounded = () => {
    behaviour.translateExtent([
      [0, 0],
      [props.laid.w, props.laid.h],
    ]);
    behaviour.scaleExtent([whole().k, CLOSEST]);
  };

  const fit = () => {
    bounded();
    select(paper).call(behaviour.transform, whole());
  };

  const onto = (spot: Spot, k: number) =>
    zoomIdentity
      .translate(pane().width / 2, pane().height / 2)
      .scale(k)
      .translate(-(spot.x + spot.w / 2), -(spot.y + spot.h / 2));

  const closer = () => {
    const spot = spotOf(props.here);
    if (spot) select(paper).call(behaviour.transform, onto(spot, CLOSEST / 2));
  };

  /* Touchpads fire wheel events far faster than we can draw, and d3's built-in wheel
   * handling is non-passive and recomputes per event, which lags. Batch deltas into one
   * scale change per frame; passive is fine since the page never scrolls. */
  let wheeled = 0;
  let towards: [number, number] = [0, 0];
  let turning = 0;

  const onWheel = (event: WheelEvent) => {
    const room = pane();
    towards = [event.clientX - room.left, event.clientY - room.top];
    wheeled += event.deltaY;
    if (turning) return;

    turning = requestAnimationFrame(() => {
      turning = 0;
      const by = Math.pow(0.9985, wheeled);
      wheeled = 0;
      behaviour.scaleBy(select(paper), by, towards);
    });
  };

  /* ---------------- what is where ---------------- */

  const spotOf = (id: Identity | null): Spot | null =>
    id === null ? null : (props.laid.at.get(id) ?? null);

  const every = (boxes: Box[]): Box[] =>
    boxes.flatMap((box) => [box, ...every(box.boxes)]);

  /* Nodes win over boxes; the innermost box wins over its parents. */
  const at = ({ x, y }: Point): Touched | null => {
    const covers = (spot: Point & { w: number; h: number }) =>
      x >= spot.x &&
      x <= spot.x + spot.w &&
      y >= spot.y &&
      y <= spot.y + spot.h;

    const node = [...props.laid.at].find(([, spot]) => covers(spot));
    if (node) {
      const [id] = node;
      return { node: id, file: props.review.definitions.get(id)?.file ?? "" };
    }

    const innermost = every(props.laid.boxes)
      .filter(covers)
      .reduce<Box | null>(
        (best, box) => (!best || box.w * box.h < best.w * best.h ? box : best),
        null,
      );
    return innermost ? { box: innermost, file: innermost.key } : null;
  };

  const pointing = (event: { clientX: number; clientY: number }) => {
    const room = pane();
    const view = seen();
    return {
      x: (event.clientX - room.left - view.x) / view.k,
      y: (event.clientY - room.top - view.y) / view.k,
    };
  };

  const onPointerMove = (event: PointerEvent) => {
    const what = at(pointing(event));
    setOver(what ? what.file : null);
    setTouching(what && "node" in what ? what.node : null);
  };
  const hint = () => {
    const id = touching();
    return id === null ? "" : (props.review.definitions.get(id)?.path ?? "");
  };

  const onClick = (event: MouseEvent) => {
    const what = at(pointing(event));
    if (what && "node" in what) props.onOpen(what.node);
  };

  /* ---------------- drawing ---------------- */

  /* Read from CSS variables so the theme stays the one place colours are defined.
   * getComputedStyle forces a style flush, so only re-read the palette on theme change. */
  const paint = createMemo((): Record<string, string> => {
    void wearing();
    const had = getComputedStyle(document.documentElement);
    const of = (name: string) => had.getPropertyValue(`--${name}`).trim();
    return {
      paper: of("paper"),
      ink: of("ink"),
      muted: of("muted"),
      faint: of("faint"),
      rule: of("rule"),
      lean: of("lean"),
      path: of("path"),
      add: of("add"),
      del: of("del"),
      chg: of("chg"),
      aff: of("muted"),
    };
  });

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
    const room = pane();
    const dense = window.devicePixelRatio || 1;
    paper.width = Math.round(room.width * dense);
    paper.height = Math.round(room.height * dense);
    paper.style.width = `${room.width}px`;
    paper.style.height = `${room.height}px`;
  };

  function draw() {
    const room = pane();
    const dense = window.devicePixelRatio || 1;
    const view = seen();

    ink.setTransform(dense, 0, 0, dense, 0, 0);
    ink.clearRect(0, 0, room.width, room.height);
    ink.setTransform(
      dense * view.k,
      0,
      0,
      dense * view.k,
      dense * view.x,
      dense * view.y,
    );
    ink.lineJoin = "round";
    ink.textBaseline = "alphabetic";

    const near = neighbours(props.review, props.here);
    for (const box of props.laid.boxes) place(box, 0);
    for (const edge of props.review.edges) leans(edge, near);
    ahead();
    for (const [id, spot] of props.laid.at) node(id, spot, near);
  }

  /* Only the innermost hovered box; highlighting ancestors or the selected node's box
   * was too visually noisy. */
  const lit = (box: Box) => box.key === over();

  function place(box: Box, deep: number) {
    const radius = Math.max(RADIUS.node, RADIUS.box - deep * RADIUS.step);
    const under = lit(box);

    /* Boxes are outline-only so nested boxes don't stack into ever-paler fills. */
    ink.setLineDash(deep ? [3, 3] : []);
    ink.lineWidth = under ? 1.4 : 1;
    ink.strokeStyle = under ? paint().muted : paint().rule;
    round(box.x, box.y, box.w, box.h, radius);
    ink.stroke();
    ink.setLineDash([]);

    ink.font = `${BOX_FONT}px ${MONO}`;
    ink.fillStyle = paint().muted;
    ink.fillText(box.label, box.x + 10, box.y + 16);

    for (const child of box.boxes) place(child, deep + 1);
  }

  function node(id: Identity, spot: Spot, near: Set<Identity>) {
    const definition = props.review.definitions.get(id);
    if (!definition) return;
    const here = id === props.here;
    const soon = id === props.next;
    const read = props.read.has(id);
    const dim = near.size > 0 && !near.has(id) && !here && !soon;
    const tint = paint()[TINT[definition.mark]];

    /* Filled with the page colour so edges drawn behind don't cross the name. */
    ink.fillStyle = paint().paper;
    round(spot.x, spot.y, spot.w, spot.h, RADIUS.node);
    ink.fill();

    const under = id === touching();
    ink.globalAlpha =
      here || soon || under
        ? 1
        : read && dim
          ? 0.42
          : read
            ? 0.6
            : dim
              ? 0.55
              : 1;

    /* Outline and name share a colour; read nodes fade via alpha so both dim together. */
    const edge = here ? paint().lean : soon ? paint().path : tint;

    ink.setLineDash(soon ? [5, 3] : []);
    ink.lineWidth = here ? 2 : soon ? 1.8 : under ? 2 : 1.2;
    ink.strokeStyle = edge;
    round(spot.x, spot.y, spot.w, spot.h, RADIUS.node);
    ink.stroke();
    ink.setLineDash([]);

    ink.font = `${FONT}px ${MONO}`;
    const mark = `${MARK[definition.mark]}`;
    ink.fillStyle = tint;
    ink.fillText(mark, spot.x + 10, spot.y + 18.5);
    ink.fillStyle = edge;
    ink.fillText(
      shorten(definition.name),
      spot.x + 10 + ink.measureText(`${mark} `).width,
      spot.y + 18.5,
    );
    ink.globalAlpha = 1;
  }

  function leans(edge: Edge, near: Set<Identity>) {
    const from = spotOf(edge.from);
    const to = spotOf(edge.to);
    if (!from || !to) return;

    const touching =
      props.here && (edge.from === props.here || edge.to === props.here);
    const upwards = to.y <= from.y;

    ink.globalAlpha = touching ? 1 : near.size ? 0.3 : 0.85;
    ink.strokeStyle = touching ? paint().lean : paint().faint;
    ink.lineWidth = touching ? 1.5 : upwards ? 1 : 1.2;
    ink.setLineDash(upwards ? [] : [4, 3]);
    ink.beginPath();
    if (upwards) bend(to, from);
    else aside(from, to);
    ink.stroke();
    ink.setLineDash([]);
    ink.globalAlpha = 1;
  }

  /* Arrow from the current definition to the next one in reading order. */
  function ahead() {
    const from = spotOf(props.here);
    const to = spotOf(props.next);
    if (!from || !to) return;

    const [leaves, arrives] = closest(from, to);
    ink.globalAlpha = 0.55;
    ink.strokeStyle = paint().path;
    ink.fillStyle = paint().path;
    ink.lineWidth = 1.8;
    ink.setLineDash([5, 3]);
    ink.beginPath();
    ink.moveTo(leaves.x, leaves.y);
    ink.lineTo(arrives.x, arrives.y);
    ink.stroke();
    ink.setLineDash([]);
    tip(leaves, arrives);
    ink.globalAlpha = 1;
  }

  function tip(from: Point, to: Point) {
    const turn = Math.atan2(to.y - from.y, to.x - from.x);
    const wide = 0.42;
    const long = 9;
    ink.beginPath();
    ink.moveTo(to.x, to.y);
    ink.lineTo(
      to.x - long * Math.cos(turn - wide),
      to.y - long * Math.sin(turn - wide),
    );
    ink.lineTo(
      to.x - long * Math.cos(turn + wide),
      to.y - long * Math.sin(turn + wide),
    );
    ink.closePath();
    ink.fill();
  }

  const round = (x: number, y: number, w: number, h: number, r: number) => {
    const tight = Math.min(r, w / 2, h / 2);
    ink.beginPath();
    ink.moveTo(x + tight, y);
    ink.arcTo(x + w, y, x + w, y + h, tight);
    ink.arcTo(x + w, y + h, x, y + h, tight);
    ink.arcTo(x, y + h, x, y, tight);
    ink.arcTo(x, y, x + w, y, tight);
    ink.closePath();
  };

  const bend = (up: Spot, down: Spot) => {
    const [x1, y1] = [up.x + up.w / 2, up.y + up.h];
    const [x2, y2] = [down.x + down.w / 2, down.y];
    const mid = (y1 + y2) / 2;
    ink.moveTo(x1, y1);
    ink.bezierCurveTo(x1, mid, x2, mid, x2, y2);
  };

  const aside = (from: Spot, to: Spot) => {
    const [x1, y1] = [from.x + from.w, from.y + from.h / 2];
    const [x2, y2] = [to.x + to.w, to.y + to.h / 2];
    const out = 34 + Math.abs(y2 - y1) * 0.2;
    ink.moveTo(x1, y1);
    ink.bezierCurveTo(x1 + out, y1, x2 + out, y2, x2, y2);
  };

  /* ---------------- keeping up ---------------- */

  const onKey = (event: KeyboardEvent) => {
    if (event.metaKey || event.altKey) return;
    if (event.key === "+" || event.key === "=")
      behaviour.scaleBy(select(paper), 1.2);
    else if (event.key === "-" || event.key === "_")
      behaviour.scaleBy(select(paper), 1 / 1.2);
    else if (event.key === "0") fit();
    else if (event.key === "1") closer();
    else return;
    event.preventDefault();
  };

  onMount(() => {
    ink = paper.getContext("2d")!;
    sized();
    select(paper).call(behaviour);
    frame.addEventListener("wheel", onWheel, { passive: true });
    fit();

    /* draw() directly, not redraw(): sizing clears the canvas, and rAF callbacks run
     * before ResizeObserver callbacks, so a deferred redraw would leave a blank frame
     * (visible as flicker while dragging the sidebar). */
    const resized = new ResizeObserver(() => {
      sized();
      bounded();
      if (seen().k < whole().k) fit();
      draw();
    });
    resized.observe(frame);

    /* A new layout gets fresh extents and a fit-all view. */
    createEffect(() => {
      void props.laid;
      untrack(fit);
    });

    document.addEventListener("keydown", onKey);
    onCleanup(() => {
      resized.disconnect();
      frame.removeEventListener("wheel", onWheel);
      document.removeEventListener("keydown", onKey);
    });
  });

  /* Every reactive input to draw() is read here so changes trigger a redraw. */
  createEffect(() => {
    void [
      props.here,
      props.next,
      props.read,
      props.review,
      props.laid,
      over(),
      touching(),
      paint(),
    ];
    redraw();
  });

  /* Pan to the current definition only when it's off screen. */
  createEffect(() => {
    const spot = spotOf(props.here);
    if (!spot || !paper) return;

    const view = seen();
    const room = pane();
    const [x, y] = [view.applyX(spot.x), view.applyY(spot.y)];
    const showing =
      x >= 0 &&
      y >= 0 &&
      x + spot.w * view.k <= room.width &&
      y + spot.h * view.k <= room.height;
    if (showing) return;

    select(paper).call(behaviour.transform, onto(spot, view.k));
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
      <canvas ref={paper} />

      {/* Keyboard and screen-reader access, since the canvas has none. */}
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

      <div class="viewkeys">
        <kbd>0</kbd> fit all · <kbd>1</kbd> zoom to current · <kbd>t</kbd>{" "}
        {wearing()}
      </div>
    </div>
  );
}

function neighbours(review: Review, here: Identity | null) {
  if (here === null) return new Set<Identity>();
  return new Set([
    here,
    ...review.edges.flatMap((edge) =>
      edge.from === here ? [edge.to] : edge.to === here ? [edge.from] : [],
    ),
  ]);
}

function closest(from: Spot, to: Spot): [Point, Point] {
  const best = faces(from)
    .flatMap((a) =>
      faces(to).map((b) => ({ far: Math.hypot(b.x - a.x, b.y - a.y), a, b })),
    )
    .reduce((best, pair) => (pair.far < best.far ? pair : best));
  return [best.a, best.b];
}

const faces = (box: Spot): Point[] => [
  { x: box.x + box.w / 2, y: box.y },
  { x: box.x + box.w / 2, y: box.y + box.h },
  { x: box.x, y: box.y + box.h / 2 },
  { x: box.x + box.w, y: box.y + box.h / 2 },
];
