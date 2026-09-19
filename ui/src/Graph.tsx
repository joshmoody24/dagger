import { createEffect, createSignal, onCleanup, onMount } from "solid-js";
import { select } from "d3-selection";
import { zoom as zooming, zoomIdentity, zoomTransform } from "d3-zoom";
import { MARK, NODE_H, RADIUS, TINT, inside, shorten } from "./review.ts";

/* Breathing room around the whole drawing when it's sat in the window. */
const EDGE = 24;
/* How far in a reader can go. Past this the text is bigger than anything worth reading. */
const CLOSEST = 4;
const FONT = 14;
const BOX_FONT = 12.5;
const MONO = "ui-monospace, SFMono-Regular, Menlo, Consolas, monospace";

/* The shape of the change: a box for every place code lives, nested as deep as the
 * grouping goes, a node for each definition, and lines for what leans on what.
 *
 * Lines run upwards, because whatever holds something up is drawn above it. A line that has
 * to run the other way is something the reader will be asked to take on faith, so it swings
 * out to the side where it can't be mistaken for the ordinary case.
 *
 * Drawn on a canvas rather than as elements. As elements this was a shape and a name for
 * every definition and a path for every line — some nine hundred things for the engine to
 * lay out, rasterise and hit-test, all of it again at each new size. Chrome has the room to
 * hide that. The webview the window is built on does not: measured on the same picture, ten
 * frames a second as elements against sixty here.
 *
 * What that costs is what a browser gives an element for free — nothing drawn here can be
 * tabbed to or read aloud. The list underneath makes up for it: the same definitions, in
 * reading order, as real buttons that nobody can see.
 *
 * One thing owns where the view is, and it's the zoom behaviour. Nothing here sets the
 * transform itself — moving the view means asking the behaviour to move, so what the reader
 * is doing and what the page wants to show can't end up disagreeing.
 */
export function Graph(props) {
  let frame;
  let paper;
  let ink;

  /* What the pointer is over: a definition, or the box around some.
   *
   * Worked out by asking where things are rather than by asking the page. That's the other
   * half of what a canvas buys — there's no element to hit-test, and no flicker when the
   * answer is a node sitting on top of the box it belongs to. */
  const [over, setOver] = createSignal(null);

  const behaviour = zooming()
    .translateExtent([[0, 0], [props.laid.w, props.laid.h]])
    /* Wheels are handled below instead. */
    .filter((event) => event.type !== "wheel" && !event.ctrlKey && !event.button)
    .on("zoom", () => redraw());

  const pane = () => frame.getBoundingClientRect();
  const seen = () => zoomTransform(paper);

  /* ---------------- where the view is ---------------- */

  /* The whole drawing, in the middle, as large as it goes. Also the furthest out a reader
   * can pull: there's nothing to see beyond the edges of the drawing. */
  const whole = () => {
    const room = pane();
    const k = Math.min(
      (room.width - 2 * EDGE) / props.laid.w,
      (room.height - 2 * EDGE) / props.laid.h,
    );
    return zoomIdentity
      .translate((room.width - props.laid.w * k) / 2, (room.height - props.laid.h * k) / 2)
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
      .translate(-(spot.x + spot.w / 2), -(spot.y + spot.h / 2));

  const closer = () => {
    const spot = spotOf(props.here);
    if (spot) select(paper).call(behaviour.transform, onto(spot, CLOSEST / 2));
  };

  /* A touchpad reports a wheel far faster than anything can be drawn, and the zoom that
   * ships with d3 works out a whole new view for each report — while holding the browser
   * up, because it has to say whether the page should scroll before the page can move.
   * Hundreds of those a second is the lag: not the drawing, the answering.
   *
   * So the reports are added up and turned into one change of size per frame, and nothing
   * is held up in the meantime — this page doesn't scroll, so there's nothing to prevent. */
  let wheeled = 0;
  let towards = [0, 0];
  let turning = 0;

  const onWheel = (event) => {
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

  /* Where a definition sits, whether or not it has a node. A module is drawn as its box
   * rather than a node of its own, so anything pointing at one has to point at the box. */
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

  const every = (boxes) => boxes.flatMap((box) => [box, ...every(box.boxes)]);
  const holding = () => (props.review.definitions.get(props.here) || {}).file;

  /* Everything a box stands over: itself, its module's file, and the same again for every
   * box inside it. A box lights for anything in any of them, which is what stacks the
   * tints as you point deeper in. */
  const covers = (box) =>
    [box.key, (box.module || {}).file, ...box.boxes.flatMap(covers)].filter(Boolean);

  /* Where pressing a box takes you: its own module if it has one, else the first definition
   * it holds at any depth. A place you can't point at reads as broken. */
  const opens = (box) => {
    if (box.module) return box.module.id;
    for (const node of inside(box)) return node.id;
    return null;
  };

  /* What's under a point, in the drawing's own units. A definition wins over the box it's
   * in, and the innermost box wins over the ones around it. */
  const at = ({ x, y }) => {
    for (const [id, spot] of props.laid.at) {
      if (x >= spot.x && x <= spot.x + spot.w && y >= spot.y && y <= spot.y + NODE_H) {
        return { node: id, file: props.review.definitions.get(id).file };
      }
    }

    let innermost = null;
    for (const box of every(props.laid.boxes)) {
      if (x < box.x || x > box.x + box.w || y < box.y || y > box.y + box.h) continue;
      if (!innermost || box.w * box.h < innermost.w * innermost.h) innermost = box;
    }
    return innermost ? { box: innermost, file: innermost.key } : null;
  };

  const pointing = (event) => {
    const room = pane();
    const view = seen();
    return {
      x: (event.clientX - room.left - view.x) / view.k,
      y: (event.clientY - room.top - view.y) / view.k,
    };
  };

  const onPointerMove = (event) => {
    const what = at(pointing(event));
    setOver(what && what.file);
    frame.style.cursor = what ? "pointer" : "grab";
    frame.title = what && what.node !== undefined ? props.review.definitions.get(what.node).path : "";
  };

  const onClick = (event) => {
    const what = at(pointing(event));
    if (!what) return;
    const id = what.node !== undefined ? what.node : opens(what.box);
    if (id !== null && id !== undefined) props.onOpen(id);
  };

  /* ---------------- drawing ---------------- */

  /* The palette, as the page has it. Read from the stylesheet so a theme is still the one
   * place colours are decided, even though nothing drawn here is styled by a rule. */
  let paint: Record<string, string> = {};
  const readPaint = () => {
    const had = getComputedStyle(document.documentElement);
    const of = (name) => had.getPropertyValue(`--${name}`).trim();
    paint = {
      raised: of("raised"), ink: of("ink"), muted: of("muted"), faint: of("faint"),
      rule: of("rule"), lean: of("lean"), path: of("path"),
      add: of("add"), del: of("del"), chg: of("chg"), aff: of("muted"),
    };
  };

  let drawing = 0;
  const redraw = () => {
    if (drawing || !ink) return;
    drawing = requestAnimationFrame(() => {
      drawing = 0;
      draw();
    });
  };

  /* A canvas has a size in pixels of its own, and it isn't the size it's shown at: on a
   * dense screen the two differ, and drawing at the wrong one is how text goes soft. */
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
    ink.setTransform(dense * view.k, 0, 0, dense * view.k, dense * view.x, dense * view.y);
    ink.lineJoin = "round";
    ink.textBaseline = "alphabetic";

    const near = neighbours(props.review, props.here);
    for (const box of props.laid.boxes) place(box, 0);
    for (const edge of props.review.edges) leans(edge, near);
    ahead();
    for (const [id, spot] of props.laid.at) node(id, spot, near);
  }

  const lit = (box) => covers(box).some((key) => key === over() || key === holding());

  function place(box, deep) {
    const radius = Math.max(RADIUS.node, RADIUS.box - deep * RADIUS.step);
    const module = box.module;

    /* Laid on with alpha rather than a flat colour, so a box inside a lit box adds to it
     * instead of replacing it: the deepest place the pointer is in is the brightest. */
    if (lit(box) && opens(box) !== null) {
      ink.globalAlpha = 0.4;
      ink.fillStyle = paint.raised;
      round(box.x, box.y, box.w, box.h, radius);
      ink.fill();
      ink.globalAlpha = 1;
    }

    const here = module && module.id === props.here;
    const soon = module && module.id === props.next;
    ink.setLineDash(soon ? [5, 3] : deep && !here ? [3, 3] : []);
    ink.lineWidth = here || soon ? 1.8 : 1;
    ink.strokeStyle = here ? paint.lean : soon ? paint.path : paint.rule;
    round(box.x, box.y, box.w, box.h, radius);
    ink.stroke();
    ink.setLineDash([]);

    ink.font = `${BOX_FONT}px ${MONO}`;
    let x = box.x + 10;
    if (module) {
      const mark = `${MARK[module.mark]} `;
      ink.fillStyle = paint[TINT[module.mark]];
      ink.fillText(mark, x, box.y + 16);
      x += ink.measureText(mark).width;
    }
    ink.fillStyle = paint.muted;
    ink.fillText(box.label, x, box.y + 16);

    for (const child of box.boxes) place(child, deep + 1);
  }

  function node(id, spot, near) {
    const definition = props.review.definitions.get(id);
    const here = id === props.here;
    const soon = id === props.next;
    const read = props.read.has(id);
    const dim = near.size > 0 && !near.has(id) && !here && !soon;
    const tint = paint[TINT[definition.mark]];

    ink.globalAlpha = here || soon ? 1 : read && dim ? 0.42 : read ? 0.6 : dim ? 0.55 : 1;

    /* A node that's been read is emptied out — it keeps its coloured edge, so what happened
     * to it is still legible, but it stops being a solid thing on the page. */
    if (!read || here) {
      ink.fillStyle = paint.raised;
      round(spot.x, spot.y, spot.w, NODE_H, RADIUS.node);
      ink.fill();
    }

    ink.setLineDash(soon ? [5, 3] : []);
    ink.lineWidth = here ? 2 : soon ? 1.8 : 1.2;
    ink.strokeStyle = here ? paint.lean : soon ? paint.path : tint;
    round(spot.x, spot.y, spot.w, NODE_H, RADIUS.node);
    ink.stroke();
    ink.setLineDash([]);

    ink.font = `${FONT}px ${MONO}`;
    const mark = `${MARK[definition.mark]}`;
    ink.fillStyle = tint;
    ink.fillText(mark, spot.x + 10, spot.y + 18.5);
    ink.fillStyle = here ? paint.lean : read ? paint.muted : paint.ink;
    ink.fillText(
      shorten(definition.name),
      spot.x + 10 + ink.measureText(`${mark} `).width,
      spot.y + 18.5,
    );
    ink.globalAlpha = 1;
  }

  function leans(edge, near) {
    const from = spotOf(edge.from);
    const to = spotOf(edge.to);
    if (!from || !to) return;

    const touching = props.here && (edge.from === props.here || edge.to === props.here);
    const upwards = to.y <= from.y;

    ink.globalAlpha = touching ? 1 : near.size ? 0.3 : 0.85;
    ink.strokeStyle = touching ? paint.lean : paint.faint;
    ink.lineWidth = touching ? 1.5 : upwards ? 1 : 1.2;
    ink.setLineDash(upwards ? [] : [4, 3]);
    ink.beginPath();
    if (upwards) bend(to, from);
    else aside(from, to);
    ink.stroke();
    ink.setLineDash([]);
    ink.globalAlpha = 1;
  }

  /* Where the reading goes next. Nothing else in the drawing has an arrowhead, because
   * nothing else is about which way time runs. */
  function ahead() {
    const from = spotOf(props.here);
    const to = spotOf(props.next);
    if (!from || !to) return;

    const [leaves, arrives] = closest(from, to);
    ink.globalAlpha = 0.55;
    ink.strokeStyle = paint.path;
    ink.fillStyle = paint.path;
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

  function tip(from, to) {
    const turn = Math.atan2(to.y - from.y, to.x - from.x);
    const wide = 0.42;
    const long = 9;
    ink.beginPath();
    ink.moveTo(to.x, to.y);
    ink.lineTo(to.x - long * Math.cos(turn - wide), to.y - long * Math.sin(turn - wide));
    ink.lineTo(to.x - long * Math.cos(turn + wide), to.y - long * Math.sin(turn + wide));
    ink.closePath();
    ink.fill();
  }

  const round = (x, y, w, h, r) => {
    const tight = Math.min(r, w / 2, h / 2);
    ink.beginPath();
    ink.moveTo(x + tight, y);
    ink.arcTo(x + w, y, x + w, y + h, tight);
    ink.arcTo(x + w, y + h, x, y + h, tight);
    ink.arcTo(x, y + h, x, y, tight);
    ink.arcTo(x, y, x + w, y, tight);
    ink.closePath();
  };

  const bend = (up, down) => {
    const [x1, y1] = [up.x + up.w / 2, up.y + up.h];
    const [x2, y2] = [down.x + down.w / 2, down.y];
    const mid = (y1 + y2) / 2;
    ink.moveTo(x1, y1);
    ink.bezierCurveTo(x1, mid, x2, mid, x2, y2);
  };

  const aside = (from, to) => {
    const [x1, y1] = [from.x + from.w, from.y + from.h / 2];
    const [x2, y2] = [to.x + to.w, to.y + to.h / 2];
    const out = 34 + Math.abs(y2 - y1) * 0.2;
    ink.moveTo(x1, y1);
    ink.bezierCurveTo(x1 + out, y1, x2 + out, y2, x2, y2);
  };

  /* ---------------- keeping up ---------------- */

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
    ink = paper.getContext("2d");
    readPaint();
    sized();
    select(paper).call(behaviour);
    frame.addEventListener("wheel", onWheel, { passive: true });
    fit();

    /* A resized window changes how far out the whole drawing sits, so the limit has to
     * move with it or the reader gets stuck too close in. */
    const resized = new ResizeObserver(() => {
      sized();
      const shown = whole();
      behaviour.scaleExtent([shown.k, CLOSEST]);
      if (seen().k < shown.k) fit();
      else redraw();
    });
    resized.observe(frame);

    document.addEventListener("keydown", onKey);
    onCleanup(() => {
      resized.disconnect();
      frame.removeEventListener("wheel", onWheel);
      document.removeEventListener("keydown", onKey);
    });
  });

  /* Everything the drawing depends on, watched in one place: read it here, and a change to
   * it draws again. */
  createEffect(() => {
    void [props.here, props.next, props.read, props.review, props.laid, over()];
    redraw();
  });

  /* Reading moves the view, but only when what's being read has gone off screen, and only
   * ever in answer to the reading. */
  createEffect(() => {
    const spot = spotOf(props.here);
    if (!spot || !paper) return;

    const view = seen();
    const room = pane();
    const [x, y] = [view.applyX(spot.x), view.applyY(spot.y)];
    const showing =
      x >= 0 && y >= 0 && x + spot.w * view.k <= room.width && y + spot.h * view.k <= room.height;
    if (showing) return;

    select(paper).call(behaviour.transform, onto(spot, view.k));
  });

  return (
    <div
      class="canvas"
      ref={frame}
      onPointerMove={onPointerMove}
      onPointerLeave={() => setOver(null)}
      onClick={onClick}
    >
      <canvas ref={paper} />

      {/* What a canvas can't be: something to tab through, and something to read aloud. */}
      <ul class="spoken" aria-label="What changed, and what holds up what">
        {props.review.steps.map((step) => {
          const definition = props.review.definitions.get(step.definition);
          return (
            <li>
              <button onClick={() => props.onOpen(definition.id)}>
                {definition.path} — {definition.kind}
              </button>
            </li>
          );
        })}
      </ul>

      <div class="viewkeys">
        scroll to zoom · drag to move · <kbd>0</kbd> fit all · <kbd>1</kbd> zoom to current
      </div>
    </div>
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

/* From the definition being read to the one after it: between whichever pair of faces sits
 * closest together. */
function closest(from, to) {
  let best = null;
  for (const a of faces(from)) {
    for (const b of faces(to)) {
      const far = Math.hypot(b.x - a.x, b.y - a.y);
      if (!best || far < best.far) best = { far, a, b };
    }
  }
  return [best.a, best.b];
}

const faces = (box) => [
  { x: box.x + box.w / 2, y: box.y },
  { x: box.x + box.w / 2, y: box.y + box.h },
  { x: box.x, y: box.y + box.h / 2 },
  { x: box.x + box.w, y: box.y + box.h / 2 },
];
