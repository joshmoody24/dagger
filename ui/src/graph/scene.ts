import type {
  Box,
  Definition,
  Edge,
  Identity,
  Laid,
  Review,
  Spot,
} from "../dagger.ts";
import { MARK, TINT } from "../digest.ts";
import { FONT, RADIUS, shorten } from "../layout.ts";

/* What the graph shows, as plain data: geometry and class names, never colours. Colours
 * live in Graph.css so a theme change needs nothing here. Plain functions so it can be
 * tested without a browser. */

export interface Input {
  review: Review;
  laid: Laid;
  here: Identity | null;
  next: Identity | null;
  read: Set<Identity>;
  /** The hovered box's key, if any. */
  over: string | null;
  /** The hovered node, if any. */
  touching: Identity | null;
  /** The toolbar filter; empty means nothing is filtered. */
  query: string;
}

/* Case-insensitive substring on any of the names a reader might have in mind. */
export const matches = (definition: Definition, query: string) => {
  const want = query.toLowerCase();
  return [definition.name, definition.path, definition.file].some((it) =>
    it.toLowerCase().includes(want),
  );
};

export interface SceneBox {
  key: string;
  label: string;
  x: number;
  y: number;
  w: number;
  h: number;
  radius: number;
  depth: number;
  lit: boolean;
}

/** `lit` touches the current definition; `dim` is any other while one is current. */
export type EdgeKind = "lit" | "dim" | "plain";

export interface SceneEdge {
  key: string;
  path: string;
  from: Identity;
  to: Identity;
  kind: EdgeKind;
  /** Points down the page: a dependency taken on faith rather than read first. */
  faith: boolean;
}

export interface SceneNode {
  id: Identity;
  x: number;
  y: number;
  w: number;
  h: number;
  name: string;
  nameX: number;
  mark: string;
  tint: string;
  title: string;
  classes: string[];
}

export interface Scene {
  boxes: SceneBox[];
  edges: SceneEdge[];
  nodes: SceneNode[];
  /** Arrow from the current definition to the next one in reading order. */
  ahead: { path: string; tip: string } | null;
}

interface Point {
  x: number;
  y: number;
}

/* Where text sits inside its box. The name follows the mark and a space, measured at the
 * layout's 0.6em per character rather than the page's actual font. */
const TEXT_X = 10;
export const NODE_TEXT_Y = 18.5;
export const BOX_TEXT_Y = 16;
const CHAR = FONT * 0.6;

export function scene(input: Input): Scene {
  /* A filter is the only thing that dims while one is typed. */
  const near = input.query
    ? new Set<Identity>()
    : neighbours(input.review, input.here);
  return {
    boxes: input.laid.boxes.flatMap((box) => placed(box, 0, input.over)),
    edges: input.review.edges.flatMap((edge, index) =>
      leans(input, edge, index, near),
    ),
    nodes: [...input.laid.at].flatMap(([id, spot]) =>
      node(input, id, spot, near),
    ),
    ahead: ahead(input),
  };
}

const spotOf = (laid: Laid, id: Identity | null): Spot | null =>
  id === null ? null : (laid.at.get(id) ?? null);

function placed(box: Box, depth: number, over: string | null): SceneBox[] {
  return [
    {
      key: box.key,
      label: box.label,
      x: box.x,
      y: box.y,
      w: box.w,
      h: box.h,
      radius: Math.max(RADIUS.node, RADIUS.box - depth * RADIUS.step),
      depth,
      /* Only the innermost hovered box; lighting ancestors or the current node's box was
       * too visually noisy. */
      lit: box.key === over,
    },
    ...box.boxes.flatMap((child) => placed(child, depth + 1, over)),
  ];
}

function node(
  input: Input,
  id: Identity,
  spot: Spot,
  near: Set<Identity>,
): SceneNode[] {
  const definition = input.review.definitions.get(id);
  if (!definition) return [];
  const here = id === input.here;
  const soon = id === input.next;
  const dim = input.query
    ? !matches(definition, input.query)
    : near.size > 0 && !near.has(id) && !here && !soon;
  const mark = MARK[definition.mark];
  const states = {
    here,
    soon,
    read: input.read.has(id),
    dim,
    under: id === input.touching,
  };
  return [
    {
      id,
      ...spot,
      name: shorten(definition.name),
      nameX: TEXT_X + (mark.length + 1) * CHAR,
      mark,
      tint: TINT[definition.mark],
      title: definition.path,
      classes: Object.entries(states).flatMap(([name, on]) =>
        on ? [name] : [],
      ),
    },
  ];
}

function leans(
  input: Input,
  edge: Edge,
  index: number,
  near: Set<Identity>,
): SceneEdge[] {
  const from = spotOf(input.laid, edge.from);
  const to = spotOf(input.laid, edge.to);
  if (!from || !to) return [];

  const touching =
    input.here !== null && (edge.from === input.here || edge.to === input.here);
  const upwards = to.y <= from.y;
  return [
    {
      key: `${index}:${edge.from}>${edge.to}`,
      path: upwards ? bend(to, from) : aside(from, to),
      from: edge.from,
      to: edge.to,
      kind: touching ? "lit" : near.size ? "dim" : "plain",
      faith: !upwards,
    },
  ];
}

function ahead(input: Input): Scene["ahead"] {
  const from = spotOf(input.laid, input.here);
  const to = spotOf(input.laid, input.next);
  if (!from || !to) return null;
  const [leaves, arrives] = closest(from, to);
  return {
    path: `M ${leaves.x} ${leaves.y} L ${arrives.x} ${arrives.y}`,
    tip: tip(leaves, arrives),
  };
}

function tip(from: Point, to: Point) {
  const turn = Math.atan2(to.y - from.y, to.x - from.x);
  const wide = 0.42;
  const long = 9;
  const wing = (side: number) =>
    `${to.x - long * Math.cos(turn + side)} ${to.y - long * Math.sin(turn + side)}`;
  return `M ${to.x} ${to.y} L ${wing(-wide)} L ${wing(wide)} Z`;
}

function bend(up: Spot, down: Spot) {
  const [x1, y1] = [up.x + up.w / 2, up.y + up.h];
  const [x2, y2] = [down.x + down.w / 2, down.y];
  const mid = (y1 + y2) / 2;
  return `M ${x1} ${y1} C ${x1} ${mid}, ${x2} ${mid}, ${x2} ${y2}`;
}

function aside(from: Spot, to: Spot) {
  const [x1, y1] = [from.x + from.w, from.y + from.h / 2];
  const [x2, y2] = [to.x + to.w, to.y + to.h / 2];
  const out = 34 + Math.abs(y2 - y1) * 0.2;
  return `M ${x1} ${y1} C ${x1 + out} ${y1}, ${x2 + out} ${y2}, ${x2} ${y2}`;
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
