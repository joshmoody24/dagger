import type {
  Box,
  Definition,
  Group,
  Identity,
  Laid,
  Review,
  Spot,
} from "./dagger.ts";

/* Pixels for the shape dagger sends.
 *
 * What nests in what, which tier each thing sits on, and the order along a tier all arrive
 * decided — the reading goes by the same numbers, and a page that worked them out again for
 * itself agreed until it didn't. What's left is geometry: how wide a box has to be for what
 * it holds, where each node lands.
 *
 * Plain functions, no components: this is the part worth testing without a browser.
 */

export const NODE_H = 28;
const ROW_GAP = 22,
  BAND_GAP = 30,
  BOX_GAP = 14;
const PAD_X = 12,
  PAD_TOP = 24,
  PAD_BOTTOM = 10,
  NODE_GAP = 10,
  MARGIN = 16;

/* How wide a character is in the graph's font, which has to agree with style.css: a
 * monospace advance is 0.6 of its size, and the boxes are drawn from this rather than
 * measured, since there's no page to measure against when this is tested. */
const FONT = 14;
const CHAR = FONT * 0.6;
/* Past this a name is cut short. A test is a sentence with underscores in it, and one of
 * them is as wide as a dozen ordinary definitions put together — which buries them. */
const LONGEST = 22;

/* Kept in one place so a node and the boxes around it can't drift apart. Each box inside
 * another is a touch tighter, down to the radius a node has — concentric corners would want
 * the padding added at every level, which on boxes this big comes out far too round. */
export const RADIUS = { node: 5, box: 9, step: 2 };

/* A box wears its name along the top, so it can't be narrower than the name. Smaller than
 * the nodes' font, and it has to agree with style.css the same way. */
const BOX_FONT = 12.5;
const labelWidth = (text: string) =>
  Math.ceil(text.length * BOX_FONT * 0.6) + 22;

/** A name as the graph shows it: long ones lose their tail rather than their box. */
export const shorten = (name: string) =>
  name.length > LONGEST ? `${name.slice(0, LONGEST - 1)}…` : name;

/* Rounded up, never down: a box half a pixel too small is a name poking out of it. The 34
 * is the space either side plus the mark and the gap after it. */
export const widthOf = (text: string) =>
  Math.ceil(shorten(text).length * CHAR) + 34;

/* One band of a box: side by side, in the order they arrived. A run of nodes is one block,
 * folded to about as wide as it is tall; a box inside is a block of its own. */
type Cell = { box: Sized } | { lines: Definition[][] };
interface Sized {
  label: string;
  bands: Cell[][];
  w: number;
  h: number;
}

export function layout(review: Review): Laid {
  const at = new Map<Identity, Spot>();

  /* Whatever the page has been asked not to show is gone from the definitions, and a box
   * left holding nothing goes with it: an empty box labelled after a module nothing in the
   * reading mentions. */
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

  /* Room for whatever a box holds, a band at a time, and never narrower than its own name. */
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
    /* A box with nothing in it is its own name and no more. */
    return {
      label: group.name,
      bands,
      w: Math.max(across + 2 * PAD_X, labelWidth(group.name)),
      h: bands.length ? PAD_TOP + deep + PAD_BOTTOM : PAD_TOP,
    };
  }
}

/* Placing a box is placing what it holds, a band at a time and along each band in turn:
 * the same job one level in for the boxes, and the end of it for the nodes. */
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

/* A row of peers folded into a block about as wide as it is tall, so a file with a dozen
 * tests in it grows downwards instead of off the side of the page. Nothing in a row leans
 * on anything else in it — an edge would have put one of them a tier lower — so they can be
 * split across lines without a line ever pointing the wrong way. */
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
/* Lines of definitions sit closer to each other than a box sits to anything. */
const gapAbove = (above: Cell[], band: Cell[]) =>
  [above, band].every((one) => one.every((cell) => "lines" in cell))
    ? ROW_GAP
    : BAND_GAP;
