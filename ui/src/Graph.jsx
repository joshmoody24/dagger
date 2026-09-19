import { createEffect, createSignal, For, onCleanup, onMount, Show } from "solid-js";
import { select } from "d3-selection";
import { zoom as zooming, zoomIdentity, zoomTransform } from "d3-zoom";
import { MARK, NODE_H, RADIUS, TINT, inside, shorten } from "./review.js";

/* Breathing room around the whole drawing when it's sat in the window. */
const EDGE = 24;
/* How far in a reader can go. Past this the text is bigger than anything worth reading. */
const CLOSEST = 4;

/* The shape of the change: a box for every place code lives, nested as deep as the
 * grouping goes, a node for each definition, and lines for what leans on what.
 *
 * Lines run upwards, because whatever holds something up is drawn above it. A line that has
 * to run the other way is something the reader will be asked to take on faith, so it swings
 * out to the side where it can't be mistaken for the ordinary case.
 *
 * The view is the reader's to move: drag to pan, scroll to zoom, and the page keeps up with
 * the reading by bringing whatever is being read into view.
 *
 * One thing owns where the view is, and it's the zoom behaviour. Nothing here sets the
 * transform itself — moving the view means asking the behaviour to move, so what the reader
 * is doing and what the page wants to show can't end up disagreeing. Getting that wrong is
 * what made zooming in far enough a trap: the page kept dragging the view back to the
 * definition being read, and every attempt to zoom out was overwritten.
 */
export function Graph(props) {
  let frame;
  let paper;
  let moving;

  /* Which module's box the pointer is inside.
   *
   * Not :hover, which only reaches what's under the pointer and its ancestors. A node sits
   * on top of its box without being inside it in the drawing, so moving across the nodes
   * made the box flicker on and off. Asking what's under the pointer and working out which
   * box it belongs to holds steady. */
  const [over, setOver] = createSignal(null);

  const onPointerOver = (event) => {
    const node = event.target.closest(".nd");
    if (node) return setOver(node.dataset.file);
    /* The innermost box wins on its own: a file's box is drawn over its group's, so this
     * finds the file when the pointer is in one and the group when it's in the space
     * around them. */
    setOver(event.target.closest(".box")?.dataset.box || null);
  };

  const behaviour = zooming()
    .translateExtent([[0, 0], [props.laid.w, props.laid.h]])
    .on("zoom", (event) => moving.setAttribute("transform", event.transform));

  const pane = () => frame.getBoundingClientRect();

  /* The whole drawing, in the middle, as large as it goes. Also the furthest out a reader
   * can pull: there's nothing to see beyond the edges of the drawing. */
  const whole = () => {
    const seen = pane();
    const k = Math.min((seen.width - 2 * EDGE) / props.laid.w, (seen.height - 2 * EDGE) / props.laid.h);
    return zoomIdentity
      .translate((seen.width - props.laid.w * k) / 2, (seen.height - props.laid.h * k) / 2)
      .scale(k);
  };

  const fit = () => {
    const shown = whole();
    behaviour.scaleExtent([shown.k, CLOSEST]);
    select(paper).call(behaviour.transform, shown);
  };

  /* The same view, moved so one definition sits in the middle of it. */
  const onto = (spot, k) =>
    zoomIdentity
      .translate(pane().width / 2, pane().height / 2)
      .scale(k)
      .translate(-(spot.x + spot.w / 2), -(spot.y + NODE_H / 2));

  const closer = () => {
    const spot = props.laid.at.get(props.here);
    if (spot) select(paper).call(behaviour.transform, onto(spot, CLOSEST / 2));
  };

  /* The box holding whatever is being read stays lit, so a glance says where you are
   * without hunting for the one outlined node. */
  const holding = () => (props.review.definitions.get(props.here) || {}).file;

  /* Where a definition sits, whether or not it has a node. A module is drawn as its file's
   * box rather than a node of its own, so anything pointing at one has to point at the box
   * — without this the arrow simply vanished whenever the reading passed through a module. */
  const spotOf = (id) => {
    const node = props.laid.at.get(id);
    if (node) return { x: node.x, y: node.y, w: node.w, h: NODE_H };

    const boxed = (box) => {
      if (box.module && box.module.id === id) {
        return { x: box.x, y: box.y, w: box.w, h: box.h };
      }
      for (const child of box.boxes) {
        const found = boxed(child);
        if (found) return found;
      }
      return null;
    };
    for (const box of props.laid.boxes) {
      const found = boxed(box);
      if (found) return found;
    }
    return null;
  };

  /* Everything a box stands over: itself, its module's file, and the same again for every
   * box inside it. A box lights for anything in any of them, which is what stacks the
   * tints as you point deeper in. */
  const covers = (box) =>
    [box.key, (box.module || {}).file, ...box.boxes.flatMap(covers)].filter(Boolean);
  const lit = (box) => covers(box).some((key) => key === over() || key === holding());

  /* Where pressing a box takes you: its own module if it has one, else the first definition
   * it holds at any depth. A place you can't point at reads as broken. */
  const opens = (box) => {
    if (box.module) return box.module.id;
    for (const node of inside(box)) return node.id;
    return null;
  };

  const near = () => neighbours(props.review, props.here);
  const dimmed = (id) => near().size > 0 && !near().has(id);
  const placed = () =>
    [...props.review.definitions.values()].filter((definition) => props.laid.at.has(definition.id));

  /* Zoom from the keyboard as well, since that's how the rest of this is driven. */
  const onKey = (event) => {
    if (event.metaKey || event.altKey) return;
    if (event.key === "+" || event.key === "=") behaviour.scaleBy(select(paper), 1.2);
    else if (event.key === "-" || event.key === "_") behaviour.scaleBy(select(paper), 1 / 1.2);
    else if (event.key === "0") fit();
    else if (event.key === "1") closer();
    else return;
    event.preventDefault();
  };

  onMount(() => {
    select(paper).call(behaviour);
    fit();

    /* A resized window changes how far out the whole drawing sits, so the limit has to
     * move with it or the reader gets stuck too close in. */
    const resized = new ResizeObserver(() => {
      const shown = whole();
      behaviour.scaleExtent([shown.k, CLOSEST]);
      if (zoomTransform(paper).k < shown.k) fit();
    });
    resized.observe(frame);

    document.addEventListener("keydown", onKey);
    onCleanup(() => {
      resized.disconnect();
      document.removeEventListener("keydown", onKey);
    });
  });

  /* Reading moves the view, but only when what's being read has gone off screen, and only
   * ever in answer to the reading. Where the view is isn't watched here — that's what let
   * this fight the reader before. */
  createEffect(() => {
    const spot = props.laid.at.get(props.here);
    if (!spot || !paper) return;

    const at = zoomTransform(paper);
    const seen = pane();
    const [x, y] = [at.applyX(spot.x), at.applyY(spot.y)];
    const showing =
      x >= 0 && y >= 0 && x + spot.w * at.k <= seen.width && y + NODE_H * at.k <= seen.height;
    if (showing) return;

    select(paper).call(behaviour.transform, onto(spot, at.k));
  });

  return (
    <div class="canvas" ref={frame} onPointerOver={onPointerOver} onPointerLeave={() => setOver(null)}>
      <svg
        class={near().size ? "focus" : ""}
        ref={paper}
        role="img"
        aria-label="What changed, and what holds up what"
      >
        <defs>
          <marker
            id="tip"
            viewBox="0 0 8 8"
            refX="7"
            refY="4"
            markerWidth="6"
            markerHeight="6"
            orient="auto-start-reverse"
          >
            <path d="M0,0 L8,4 L0,8 z" />
          </marker>
        </defs>
        <g ref={moving}>
          <For each={props.laid.boxes}>
            {(box) => (
              <Box
                box={box}
                deep={0}
                radius={RADIUS.box}
                here={props.here}
                next={props.next}
                lit={lit}
                opens={opens}
                onOpen={props.onOpen}
              />
            )}
          </For>

          <For each={props.review.edges}>
            {(edge) => <Leans edge={edge} at={props.laid.at} here={props.here} />}
          </For>

          <For each={placed()}>
            {(definition) => (
              <Node
                definition={definition}
                spot={props.laid.at.get(definition.id)}
                here={props.here}
                next={props.next}
                read={props.read.has(definition.id)}
                dim={dimmed(definition.id)}
                file={definition.file}
                onOpen={props.onOpen}
              />
            )}
          </For>

          {/* Where the reading goes next. The order is the whole point of the tool, and
            * without this the graph shows where you are but not where you're being taken. */}
          <Show when={spotOf(props.here) && spotOf(props.next)}>
            <path class="up" d={onward(spotOf(props.here), spotOf(props.next))} />
          </Show>
        </g>
      </svg>

      <div class="viewkeys">
        scroll to zoom · drag to move · <kbd>0</kbd> fit all · <kbd>1</kbd> zoom to current
      </div>
    </div>
  );
}

/* A box, and whatever it holds — which is boxes, so this draws itself again one level in.
 *
 * There is no such thing here as a group or a file, only a place at some depth. Each takes
 * a press, each lights when the pointer or the reading is anywhere inside it, and each that
 * answers to a module wears that module's mark and its current-or-next outline. Because the
 * tints are laid on with alpha, a box inside a lit box adds to it rather than replacing it,
 * and the deepest place you're pointing is the brightest thing on the page.
 */
function Box(props) {
  const module = () => props.box.module;
  const mine = () => module() && module().id === props.here;
  const soon = () => module() && module().id === props.next;

  const classes = () =>
    ["box", props.deep ? "deep" : "", mine() ? "here" : "", soon() ? "next" : "",
      module() ? TINT[module().mark] : ""]
      .filter(Boolean)
      .join(" ");

  return (
    <g class={classes()} data-box={props.box.key}>
      <Show when={props.opens(props.box)}>
        <rect
          class={`hit${props.lit(props.box) ? " on" : ""}`}
          x={props.box.x}
          y={props.box.y}
          width={props.box.w}
          height={props.box.h}
          rx={props.radius}
          onClick={() => props.onOpen(props.opens(props.box))}
        />
      </Show>
      <rect
        x={props.box.x}
        y={props.box.y}
        width={props.box.w}
        height={props.box.h}
        rx={props.radius}
      />
      <text x={props.box.x + 10} y={props.box.y + 16}>
        <Show when={module()}>
          <tspan class="mk" font-weight="700">{MARK[module().mark]} </tspan>
        </Show>
        {props.box.label}
      </text>

      <For each={props.box.boxes}>
        {(child) => (
          <Box
            box={child}
            deep={props.deep + 1}
            radius={Math.max(RADIUS.node, props.radius - RADIUS.step)}
            here={props.here}
            next={props.next}
            lit={props.lit}
            opens={props.opens}
            onOpen={props.onOpen}
          />
        )}
      </For>
    </g>
  );
}

function Node(props) {
  const marking = () => props.definition.mark;
  const classes = () =>
    ["nd",
      TINT[marking()],
      props.definition.id === props.here ? "sel" : "",
      props.definition.id === props.next ? "next" : "",
      props.read ? "done" : "",
      props.dim ? "dim" : ""]
      .filter(Boolean)
      .join(" ");

  return (
    <g
      class={classes()}
      transform={`translate(${props.spot.x},${props.spot.y})`}
      data-file={props.file}
      tabindex="0"
      role="button"
      aria-label={props.definition.path}
      onClick={() => props.onOpen(props.definition.id)}
      onKeyDown={(event) => event.key === "Enter" && props.onOpen(props.definition.id)}
    >
      <rect width={props.spot.w} height={NODE_H} rx={RADIUS.node} />
      <text x="10" y="18.5">
        <tspan class="mk" font-weight="700">{MARK[marking()]}</tspan>
        <tspan class="nm" dx="6">{shorten(props.definition.name)}</tspan>
      </text>
      <title>{props.definition.path}</title>
    </g>
  );
}

function Leans(props) {
  const from = () => props.at.get(props.edge.from);
  const to = () => props.at.get(props.edge.to);
  const touching = () => props.here && (props.edge.from === props.here || props.edge.to === props.here);
  const upwards = () => to().y <= from().y;
  const shape = () => (upwards() ? bend(to(), from()) : aside(from(), to()));

  return (
    <Show when={from() && to()}>
      <path class={`${upwards() ? "eg" : "ec"}${touching() ? " on" : ""}`} d={shape()} />
      <Show when={touching()}>
        <path class="eh" d={shape()} />
      </Show>
    </Show>
  );
}

function neighbours(review, here) {
  const near = new Set();
  if (!here) return near;
  near.add(here);
  for (const edge of review.edges) {
    if (edge.from === here) near.add(edge.to);
    if (edge.to === here) near.add(edge.from);
  }
  return near;
}

/* From the definition being read to the one after it: straight, between whichever pair of
 * faces sits closest together. Picking a side by which way the target mostly lies gets it
 * wrong whenever two boxes are roughly level or one wraps around the other — the line then
 * sets off away from where it's going before crossing back. Trying all sixteen pairs and
 * keeping the shortest is both simpler to say and always right. */
function onward(from, to) {
  let best = null;
  for (const a of faces(from)) {
    for (const b of faces(to)) {
      const far = Math.hypot(b.x - a.x, b.y - a.y);
      if (!best || far < best.far) best = { far, a, b };
    }
  }
  return `M${best.a.x},${best.a.y} L${best.b.x},${best.b.y}`;
}

const faces = (box) => [
  { x: box.x + box.w / 2, y: box.y },
  { x: box.x + box.w / 2, y: box.y + box.h },
  { x: box.x, y: box.y + box.h / 2 },
  { x: box.x + box.w, y: box.y + box.h / 2 },
];

const bend = (up, down) => {
  const [x1, y1] = [up.x + up.w / 2, up.y + NODE_H];
  const [x2, y2] = [down.x + down.w / 2, down.y];
  const mid = (y1 + y2) / 2;
  return `M${x1},${y1} C${x1},${mid} ${x2},${mid} ${x2},${y2}`;
};

const aside = (from, to) => {
  const [x1, y1] = [from.x + from.w, from.y + NODE_H / 2];
  const [x2, y2] = [to.x + to.w, to.y + NODE_H / 2];
  const out = 34 + Math.abs(y2 - y1) * 0.2;
  return `M${x1},${y1} C${x1 + out},${y1} ${x2 + out},${y2} ${x2},${y2}`;
};
