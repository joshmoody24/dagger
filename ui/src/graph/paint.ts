import type { Box, Edge, Identity, Laid, Review, Spot } from "../dagger.ts";
import { MARK, TINT } from "../digest.ts";
import { BOX_FONT, FONT, RADIUS, shorten } from "../layout.ts";
import type { Point } from "./hit.ts";

const MONO = "ui-monospace, SFMono-Regular, Menlo, Consolas, monospace";

export type Palette = Record<string, string>;

/* Read from CSS variables so the theme stays the one place colours are defined. */
export function palette(had: CSSStyleDeclaration): Palette {
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
}

export interface Scene {
  review: Review;
  laid: Laid;
  here: Identity | null;
  next: Identity | null;
  read: Set<Identity>;
  /** The hovered box's key, if any. */
  over: string | null;
  /** The hovered node, if any. */
  touching: Identity | null;
  view: { k: number; x: number; y: number };
  room: { width: number; height: number };
  /** devicePixelRatio: the backing store is scaled by it, or text is blurry. */
  dense: number;
}

export function paint(
  ink: CanvasRenderingContext2D,
  scene: Scene,
  tint: Palette,
) {
  const { view, room, dense } = scene;

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

  const near = neighbours(scene.review, scene.here);
  for (const box of scene.laid.boxes) place(ink, scene, tint, box, 0);
  for (const edge of scene.review.edges) leans(ink, scene, tint, edge, near);
  ahead(ink, scene, tint);
  for (const [id, spot] of scene.laid.at)
    node(ink, scene, tint, id, spot, near);
}

const spotOf = (laid: Laid, id: Identity | null): Spot | null =>
  id === null ? null : (laid.at.get(id) ?? null);

function place(
  ink: CanvasRenderingContext2D,
  scene: Scene,
  tint: Palette,
  box: Box,
  deep: number,
) {
  const radius = Math.max(RADIUS.node, RADIUS.box - deep * RADIUS.step);
  /* Only the innermost hovered box; highlighting ancestors or the selected node's box
   * was too visually noisy. */
  const under = box.key === scene.over;

  /* Boxes are outline-only so nested boxes don't stack into ever-paler fills. */
  ink.setLineDash(deep ? [3, 3] : []);
  ink.lineWidth = under ? 1.4 : 1;
  ink.strokeStyle = under ? tint.muted : tint.rule;
  round(ink, box.x, box.y, box.w, box.h, radius);
  ink.stroke();
  ink.setLineDash([]);

  ink.font = `${BOX_FONT}px ${MONO}`;
  ink.fillStyle = tint.muted;
  ink.fillText(box.label, box.x + 10, box.y + 16);

  for (const child of box.boxes) place(ink, scene, tint, child, deep + 1);
}

function node(
  ink: CanvasRenderingContext2D,
  scene: Scene,
  tint: Palette,
  id: Identity,
  spot: Spot,
  near: Set<Identity>,
) {
  const definition = scene.review.definitions.get(id);
  if (!definition) return;
  const here = id === scene.here;
  const soon = id === scene.next;
  const read = scene.read.has(id);
  const dim = near.size > 0 && !near.has(id) && !here && !soon;
  const own = tint[TINT[definition.mark]];

  /* Filled with the page colour so edges drawn behind don't cross the name. */
  ink.fillStyle = tint.paper;
  round(ink, spot.x, spot.y, spot.w, spot.h, RADIUS.node);
  ink.fill();

  const under = id === scene.touching;
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
  const edge = here ? tint.lean : soon ? tint.path : own;

  ink.setLineDash(soon ? [5, 3] : []);
  ink.lineWidth = here ? 2 : soon ? 1.8 : under ? 2 : 1.2;
  ink.strokeStyle = edge;
  round(ink, spot.x, spot.y, spot.w, spot.h, RADIUS.node);
  ink.stroke();
  ink.setLineDash([]);

  ink.font = `${FONT}px ${MONO}`;
  const mark = `${MARK[definition.mark]}`;
  ink.fillStyle = own;
  ink.fillText(mark, spot.x + 10, spot.y + 18.5);
  ink.fillStyle = edge;
  ink.fillText(
    shorten(definition.name),
    spot.x + 10 + ink.measureText(`${mark} `).width,
    spot.y + 18.5,
  );
  ink.globalAlpha = 1;
}

function leans(
  ink: CanvasRenderingContext2D,
  scene: Scene,
  tint: Palette,
  edge: Edge,
  near: Set<Identity>,
) {
  const from = spotOf(scene.laid, edge.from);
  const to = spotOf(scene.laid, edge.to);
  if (!from || !to) return;

  const touching =
    scene.here && (edge.from === scene.here || edge.to === scene.here);
  const upwards = to.y <= from.y;

  ink.globalAlpha = touching ? 1 : near.size ? 0.3 : 0.85;
  ink.strokeStyle = touching ? tint.lean : tint.faint;
  ink.lineWidth = touching ? 1.5 : upwards ? 1 : 1.2;
  ink.setLineDash(upwards ? [] : [4, 3]);
  ink.beginPath();
  if (upwards) bend(ink, to, from);
  else aside(ink, from, to);
  ink.stroke();
  ink.setLineDash([]);
  ink.globalAlpha = 1;
}

/* Arrow from the current definition to the next one in reading order. */
function ahead(ink: CanvasRenderingContext2D, scene: Scene, tint: Palette) {
  const from = spotOf(scene.laid, scene.here);
  const to = spotOf(scene.laid, scene.next);
  if (!from || !to) return;

  const [leaves, arrives] = closest(from, to);
  ink.globalAlpha = 0.55;
  ink.strokeStyle = tint.path;
  ink.fillStyle = tint.path;
  ink.lineWidth = 1.8;
  ink.setLineDash([5, 3]);
  ink.beginPath();
  ink.moveTo(leaves.x, leaves.y);
  ink.lineTo(arrives.x, arrives.y);
  ink.stroke();
  ink.setLineDash([]);
  tip(ink, leaves, arrives);
  ink.globalAlpha = 1;
}

function tip(ink: CanvasRenderingContext2D, from: Point, to: Point) {
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

function round(
  ink: CanvasRenderingContext2D,
  x: number,
  y: number,
  w: number,
  h: number,
  r: number,
) {
  const tight = Math.min(r, w / 2, h / 2);
  ink.beginPath();
  ink.moveTo(x + tight, y);
  ink.arcTo(x + w, y, x + w, y + h, tight);
  ink.arcTo(x + w, y + h, x, y + h, tight);
  ink.arcTo(x, y + h, x, y, tight);
  ink.arcTo(x, y, x + w, y, tight);
  ink.closePath();
}

function bend(ink: CanvasRenderingContext2D, up: Spot, down: Spot) {
  const [x1, y1] = [up.x + up.w / 2, up.y + up.h];
  const [x2, y2] = [down.x + down.w / 2, down.y];
  const mid = (y1 + y2) / 2;
  ink.moveTo(x1, y1);
  ink.bezierCurveTo(x1, mid, x2, mid, x2, y2);
}

function aside(ink: CanvasRenderingContext2D, from: Spot, to: Spot) {
  const [x1, y1] = [from.x + from.w, from.y + from.h / 2];
  const [x2, y2] = [to.x + to.w, to.y + to.h / 2];
  const out = 34 + Math.abs(y2 - y1) * 0.2;
  ink.moveTo(x1, y1);
  ink.bezierCurveTo(x1 + out, y1, x2 + out, y2, x2, y2);
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
