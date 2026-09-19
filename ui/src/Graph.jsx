import { createEffect, createSignal, For, onCleanup, onMount, Show } from "solid-js";
import { select } from "d3-selection";
import { zoom as zooming, zoomIdentity, zoomTransform } from "d3-zoom";
import { MARK, NODE_H, TINT, shorten } from "./review.js";

/* Breathing room around the whole drawing when it's sat in the window. */
const EDGE = 24;
/* How far in a reader can go. Past this the text is bigger than anything worth reading. */
const CLOSEST = 4;

/* The shape of the change: boxes for groups, boxes for files, a node for each definition,
 * and lines for what leans on what.
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
    const box = event.target.closest(".box.file");
    setOver((node || box)?.dataset.file || null);
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
        <g ref={moving}>
          <For each={props.laid.boxes}>
            {(box) => (
              <>
                <g class="box">
                  <rect x={box.x} y={box.y} width={box.w} height={box.h} rx="8" />
                  <text x={box.x + 10} y={box.y + 16}>{box.label}</text>
                </g>
                <For each={box.files}>
                  {(file) => (
                    <FileBox
                      file={file}
                      here={props.here}
                      over={over()}
                      holds={holding() === file.key}
                      onOpen={props.onOpen}
                    />
                  )}
                </For>
              </>
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
                read={props.read.has(definition.id)}
                dim={dimmed(definition.id)}
                file={definition.file}
                onOpen={props.onOpen}
              />
            )}
          </For>
        </g>
      </svg>

      <div class="viewkeys">
        scroll to zoom · drag to move · <kbd>0</kbd> fit all · <kbd>1</kbd> zoom to current
      </div>
    </div>
  );
}

/* A file's box wears its module's name, because a module is its file: drawing a node inside
 * the box to stand for the box would be saying the same thing twice. */
function FileBox(props) {
  const module = () => props.file.module;
  const mine = () => module() && module().id === props.here;

  return (
    <g
      class={`box file${mine() ? " here" : ""}${module() ? ` ${TINT[module().mark]}` : ""}`}
      data-file={props.file.key}
    >
      {/* A module is the only definition with no node to press, so its whole box is the way
        * in. This sits under everything else in the drawing, so lighting it up on hover
        * tints the ground without touching what's drawn on top of it — and a click landing
        * on a node inside reaches the node, never this. */}
      <Show when={module()}>
        <rect
          class={`hit${props.over === props.file.key || props.holds ? " on" : ""}`}
          x={props.file.x}
          y={props.file.y}
          width={props.file.w}
          height={props.file.h}
          rx="6"
          onClick={() => props.onOpen(module().id)}
        />
      </Show>
      <rect x={props.file.x} y={props.file.y} width={props.file.w} height={props.file.h} rx="6" />
      <text x={props.file.x + 10} y={props.file.y + 15}>
        <Show when={module()}>
          <tspan class="mk" font-weight="700">{MARK[module().mark]} </tspan>
        </Show>
        {props.file.label}
      </text>
    </g>
  );
}

function Node(props) {
  const marking = () => props.definition.mark;
  const classes = () =>
    ["nd",
      TINT[marking()],
      props.definition.id === props.here ? "sel" : "",
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
      <rect width={props.spot.w} height={NODE_H} rx="5" />
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
