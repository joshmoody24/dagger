import type {
  Box,
  Definition,
  Group,
  Identity,
  Laid,
  Review,
  Spot,
} from "./dagger.ts";

/* Geometry for the shape dagger sends. Nesting, tiers and order along a tier arrive
 * decided; this only works out widths and positions. Plain functions so it can be tested
 * without a browser. */

export const NODE_H = 28;
const ROW_GAP = 22,
  BAND_GAP = 30,
  BOX_GAP = 14;
const PAD_X = 12,
  PAD_TOP = 24,
  PAD_BOTTOM = 10,
  NODE_GAP = 10,
  MARGIN = 16;

/* Must agree with style.css. Monospace advance is 0.6 of the font size; widths are computed
 * rather than measured so tests can run without a page. */
const FONT = 14;
const CHAR = FONT * 0.6;
/* Test names are sentences with underscores; one can be as wide as a dozen ordinary nodes. */
const LONGEST = 22;

/* Each nested box is slightly tighter, down to the node radius. Concentric corners (padding
 * added at every level) come out far too round at this size. */
export const RADIUS = { node: 5, box: 9, step: 2 };

/* A box can't be narrower than its label. Must agree with style.css. */
const BOX_FONT = 12.5;
const labelWidth = (text: string) =>
  Math.ceil(text.length * BOX_FONT * 0.6) + 22;

/** A name as the graph shows it: long ones lose their tail rather than their box. */
export const shorten = (name: string) =>
  name.length > LONGEST ? `${name.slice(0, LONGEST - 1)}…` : name;

/* Rounded up: half a pixel short and the name pokes out. 34 is the side padding plus the
 * mark and its gap. */
export const widthOf = (text: string) =>
  Math.ceil(shorten(text).length * CHAR) + 34;

/* One band of a box. A run of nodes is folded into one block; a nested box is a cell of its
 * own. */
type Cell = { box: Sized } | { lines: Definition[][] };
interface Sized {
  label: string;
  bands: Cell[][];
  w: number;
  h: number;
}

export function layout(review: Review): Laid {
  const at = new Map<Identity, Spot>();

  const kept = (group: Group): Group | null => {
    if (group.type === "node")
      return review.definitions.has(group.id) ? group : null;
    const children = group.children.flatMap((child) => kept(child) ?? []);
    return children.length ? { ...group, children } : null;
  };
  const shown = review.groups.flatMap((group) => kept(group) ?? []);
  const top = sized({ type: "group", name: "", tier: 0, children: shown });

  let y = MARGIN;
  let widest = 0;
  const boxes: Box[] = [];
  for (const [index, band] of top.bands.entries()) {
    if (index) y += gapAbove(top.bands[index - 1], band);
    let x = MARGIN;
    for (const cell of band) {
      if ("box" in cell) boxes.push(placed(cell.box, x, y, "", at));
      else spots(cell.lines, x, y, at);
      x += cellWidth(cell) + BOX_GAP;
    }
    widest = Math.max(widest, x - BOX_GAP + MARGIN);
    y += bandHeight(band);
  }

  return { at, boxes, w: widest, h: y + MARGIN };

  function sized(group: Group & { type: "group" }): Sized {
    const bands: Cell[][] = [];
    let tier = -1;
    for (const child of group.children) {
      if (child.tier !== tier) bands.push([]);
      tier = child.tier;
      const band = bands[bands.length - 1];
      if (child.type === "group") band.push({ box: sized(child) });
      else {
        const node = review.definitions.get(child.id)!;
        const last = band[band.length - 1];
        if (last && "lines" in last)
          last.lines = folded([...last.lines.flat(), node]);
        else band.push({ lines: [[node]] });
      }
    }

    const across = bands.length ? Math.max(...bands.map(bandWidth)) : 0;
    const deep = bands.reduce(
      (sum, band, index) =>
        sum + bandHeight(band) + (index ? gapAbove(bands[index - 1], band) : 0),
      0,
    );
    /* An empty box is just its label. */
    return {
      label: group.name,
      bands,
      w: Math.max(across + 2 * PAD_X, labelWidth(group.name)),
      h: bands.length ? PAD_TOP + deep + PAD_BOTTOM : PAD_TOP,
    };
  }
}

function placed(
  box: Sized,
  x: number,
  y: number,
  above: string,
  at: Map<Identity, Spot>,
): Box {
  const key = `${above}/${box.label}`;
  const boxes: Box[] = [];
  const nodes: Identity[] = [];
  let down = y + PAD_TOP;

  for (const [index, band] of box.bands.entries()) {
    if (index) down += gapAbove(box.bands[index - 1], band);
    // Centred, so a lone definition under a wide band sits beneath it, not in a corner.
    let across = x + PAD_X + (box.w - 2 * PAD_X - bandWidth(band)) / 2;
    for (const cell of band) {
      if ("box" in cell) boxes.push(placed(cell.box, across, down, key, at));
      else nodes.push(...spots(cell.lines, across, down, at));
      across += cellWidth(cell) + BOX_GAP;
    }
    down += bandHeight(band);
  }

  return { key, label: box.label, x, y, w: box.w, h: box.h, boxes, nodes };
}

function spots(
  lines: Definition[][],
  x: number,
  y: number,
  at: Map<Identity, Spot>,
): Identity[] {
  let line = y;
  for (const nodes of lines) {
    let along = x;
    for (const node of nodes) {
      at.set(node.id, { x: along, y: line, w: widthOf(node.name) });
      along += widthOf(node.name) + NODE_GAP;
    }
    line += NODE_H + ROW_GAP;
  }
  return lines.flat().map((node) => node.id);
}

/* Fold a row into a roughly square block so a file with a dozen tests grows down, not
 * sideways. Nothing in a row depends on anything else in it (an edge would have put one a
 * tier lower), so splitting it can't make an edge point the wrong way. */
function folded(row: Definition[]) {
  const across = Math.ceil(Math.sqrt(row.length));
  const lines = [];
  for (let at = 0; at < row.length; at += across)
    lines.push(row.slice(at, at + across));
  return lines;
}

const rowWidth = (row: Definition[]) =>
  row.reduce((sum, n) => sum + widthOf(n.name), 0) +
  NODE_GAP * (row.length - 1);
const cellWidth = (cell: Cell) =>
  "box" in cell ? cell.box.w : Math.max(...cell.lines.map(rowWidth));
const cellHeight = (cell: Cell) =>
  "box" in cell
    ? cell.box.h
    : cell.lines.length * NODE_H + ROW_GAP * (cell.lines.length - 1);
const bandWidth = (band: Cell[]) =>
  band.reduce((sum, cell) => sum + cellWidth(cell), 0) +
  BOX_GAP * (band.length - 1);
const bandHeight = (band: Cell[]) => Math.max(...band.map(cellHeight));
/* Rows of nodes sit closer together than anything sits to a box. */
const gapAbove = (above: Cell[], band: Cell[]) =>
  [above, band].every((one) => one.every((cell) => "lines" in cell))
    ? ROW_GAP
    : BAND_GAP;
