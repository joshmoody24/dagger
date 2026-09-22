import type {
  Placed,
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
export const FONT = 14;
const CHAR = FONT * 0.6;
/* Test names are sentences with underscores; one can be as wide as a dozen ordinary definitions. */
const LONGEST = 22;

/* Each nested group is slightly tighter, down to the definition radius. Concentric corners (padding
 * added at every level) come out far too round at this size. */
export const RADIUS = { definition: 5, group: 9, step: 2 };

/* A group can't be narrower than its label. Must agree with style.css. */
export const BOX_FONT = 12.5;
const labelWidth = (text: string) =>
  Math.ceil(text.length * BOX_FONT * 0.6) + 22;

/** A name as the graph shows it: long ones lose their tail rather than their width. */
export const shorten = (name: string) =>
  name.length > LONGEST ? `${name.slice(0, LONGEST - 1)}…` : name;

/* Rounded up: half a pixel short and the name pokes out. 34 is the side padding plus the
 * mark and its gap. */
export const widthOf = (text: string) =>
  Math.ceil(shorten(text).length * CHAR) + 34;

/* One band of a group. A run of definitions is folded into one block; a nested group is a cell of its
 * own. */
type Cell = { group: Sized } | { lines: Definition[][] };
interface Sized {
  label: string;
  bands: Cell[][];
  w: number;
  h: number;
}

const rows = (cell: Cell): cell is { lines: Definition[][] } => "lines" in cell;

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

  const { groups } = stacked(top.bands, MARGIN, MARGIN, "", at, null);
  const { across, deep } = extent(top.bands);
  return {
    at,
    groups,
    w: top.bands.length ? across + 2 * MARGIN : 0,
    h: deep + 2 * MARGIN,
  };

  function sized(group: Group & { type: "group" }): Sized {
    const cell = (child: Group): Cell =>
      child.type === "group"
        ? { group: sized(child) }
        : { lines: [[review.definitions.get(child.id)!]] };
    const bands = runs(group.children, (a, b) => a.tier === b.tier).map(
      (band) =>
        runs(band.map(cell), (a, b) => rows(a) && rows(b)).map((run) =>
          run.every(rows)
            ? { lines: folded(run.flatMap((one) => one.lines.flat())) }
            : run[0],
        ),
    );

    const { across, deep } = extent(bands);
    /* An empty group is just its label. */
    return {
      label: group.name,
      bands,
      w: Math.max(across + 2 * PAD_X, labelWidth(group.name)),
      h: bands.length ? PAD_TOP + deep + PAD_BOTTOM : PAD_TOP,
    };
  }
}

/* Neighbours that belong together, kept together. */
function runs<T>(items: T[], joins: (a: T, b: T) => boolean): T[][] {
  return items.reduce<T[][]>((runs, item) => {
    const last = runs.at(-1);
    if (last && joins(last[last.length - 1], item)) last.push(item);
    else runs.push([item]);
    return runs;
  }, []);
}

/* Bands one below the last, each centred within `width` when there is one. Returns what
 * was placed directly in them. */
function stacked(
  bands: Cell[][],
  x: number,
  y: number,
  key: string,
  at: Map<Identity, Spot>,
  width: number | null,
) {
  const groups: Placed[] = [];
  const definitions: Identity[] = [];
  let down = y;

  for (const [index, band] of bands.entries()) {
    if (index) down += gapAbove(bands[index - 1], band);
    // Centred, so a lone definition under a wide band sits beneath it, not in a corner.
    let across = width === null ? x : x + (width - bandWidth(band)) / 2;
    for (const cell of band) {
      if ("group" in cell)
        groups.push(placed(cell.group, across, down, key, at));
      else definitions.push(...spots(cell.lines, across, down, at));
      across += cellWidth(cell) + BOX_GAP;
    }
    down += bandHeight(band);
  }

  return { groups, definitions };
}

function placed(
  group: Sized,
  x: number,
  y: number,
  above: string,
  at: Map<Identity, Spot>,
): Placed {
  const key = `${above}/${group.label}`;
  const { groups, definitions } = stacked(
    group.bands,
    x + PAD_X,
    y + PAD_TOP,
    key,
    at,
    group.w - 2 * PAD_X,
  );
  return {
    key,
    label: group.label,
    x,
    y,
    w: group.w,
    h: group.h,
    groups,
    definitions,
  };
}

function spots(
  lines: Definition[][],
  x: number,
  y: number,
  at: Map<Identity, Spot>,
): Identity[] {
  let line = y;
  for (const definitions of lines) {
    let along = x;
    for (const definition of definitions) {
      at.set(definition.id, {
        x: along,
        y: line,
        w: widthOf(definition.name),
        h: NODE_H,
      });
      along += widthOf(definition.name) + NODE_GAP;
    }
    line += NODE_H + ROW_GAP;
  }
  return lines.flat().map((definition) => definition.id);
}

/* Fold a row into a roughly square block so a file with a dozen tests grows down, not
 * sideways. Nothing in a row depends on anything else in it (an edge would have put one a
 * tier lower), so splitting it can't make an edge point the wrong way. */
function folded(row: Definition[]) {
  const across = Math.ceil(Math.sqrt(row.length));
  return Array.from({ length: Math.ceil(row.length / across) }, (_, line) =>
    row.slice(line * across, (line + 1) * across),
  );
}

const rowWidth = (row: Definition[]) =>
  row.reduce((sum, n) => sum + widthOf(n.name), 0) +
  NODE_GAP * (row.length - 1);
const cellWidth = (cell: Cell) =>
  "group" in cell ? cell.group.w : Math.max(...cell.lines.map(rowWidth));
const cellHeight = (cell: Cell) =>
  "group" in cell
    ? cell.group.h
    : cell.lines.length * NODE_H + ROW_GAP * (cell.lines.length - 1);
const bandWidth = (band: Cell[]) =>
  band.reduce((sum, cell) => sum + cellWidth(cell), 0) +
  BOX_GAP * (band.length - 1);
const bandHeight = (band: Cell[]) => Math.max(...band.map(cellHeight));
/* Rows of definitions sit closer together than anything sits to a group. */
const gapAbove = (above: Cell[], band: Cell[]) =>
  [above, band].every((one) => one.every((cell) => "lines" in cell))
    ? ROW_GAP
    : BAND_GAP;
/* How far a stack of bands reaches across and down. */
const extent = (bands: Cell[][]) => ({
  across: bands.length ? Math.max(...bands.map(bandWidth)) : 0,
  deep: bands.reduce(
    (sum, band, index) =>
      sum + bandHeight(band) + (index ? gapAbove(bands[index - 1], band) : 0),
    0,
  ),
});
