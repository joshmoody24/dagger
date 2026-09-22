import type {
  Box,
  Definition,
  Edge,
  Identity,
  Laid,
  Review,
  Spot,
} from "./dagger.ts";

/* Boxes inside boxes, all the way down.
 *
 * A box is a place: it holds definitions of its own, and it holds other boxes. A group and
 * a file are the same thing at different depths, so nothing below tells them apart — a box
 * is sized, placed, lit and pressed the same way wherever it sits. Whatever holds something
 * else up is drawn above it, so the page reads downwards the way the review does.
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
/* Room for a box's name and nothing else, which is all an empty one needs. */
const LABEL = 26;

/* Louder first, so a row of nodes reads worst-first when nothing else decides the order. */
const LOUDNESS = [
  "contract",
  "removed",
  "added",
  "body",
  "docs",
  "affected",
  "still",
];

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

/* Everything with no package above it: a script at the top of the repository, a config
 * file. They have nothing to do with each other, which is the point of keeping them in one
 * place — a reader already looking at odds and ends may as well see the rest of them than
 * keep coming back between packages. */
const MISC = "misc";

/* What to call a module whose own definition isn't in this review, so its name never got
 * reported. The file it lives in is the best guess left, minus the extension — which is
 * about the file on disk, not about the code. */
const guessed = (path: string) =>
  (path.split("/").at(-1) ?? path).replace(/\.[^.]+$/, "");

/* A module is its place, so it gives its box a name rather than taking a node inside the
 * box that stands for the box. */
export function layout(review: Review): Laid {
  /* A container is drawn as the box around what it holds, so it never takes a node of its
   * own. Said by whoever read the file — an impl block is a container in Rust and nothing
   * downstream could have known that. */
  const all = [...review.definitions.values()];
  const nodes = all.filter((one) => one.role !== "container");

  /* The containers a file holds outermost: the ones whose own container is somewhere else,
   * or nowhere. Usually one — a file is a module and everything in it is inside that — but
   * a language where a file is just a place to put things can have several, and each is a
   * box of its own. Keeping only one of them drew the last and lost the rest, along with
   * every box inside them. */
  const held = new Map<Identity, Definition>(all.map((one) => [one.id, one]));
  const outermost = (one: Definition): Definition => {
    const up = one.parent === null ? undefined : held.get(one.parent);
    return up && up.file === one.file ? outermost(up) : one;
  };
  const roots = collect(
    all.filter((one) => one.role === "container" && outermost(one) === one),
    (one) => one.file,
  );
  /* Named for what the page calls a file's box, which is a module where there is one. */
  const modules = new Map<string, Definition>(
    [...roots].flatMap(([file, held]) =>
      held.length === 1 ? [[file, held[0]]] : [],
    ),
  );

  const leansOn = new Map<Identity, Identity[]>(nodes.map((n) => [n.id, []]));
  for (const edge of review.edges) {
    if (leansOn.has(edge.to)) leansOn.get(edge.from)?.push(edge.to);
  }

  /* Where each definition falls in the reading. The page can't always put things in that
   * order — what holds something up is drawn above it, and that's what the lines mean —
   * but wherever the shape leaves a choice, the choice goes to the reading. A reader
   * working down the list shouldn't have their eye thrown across the page and back. */
  const reading = new Map(
    review.steps.map((step, at) => [step.definition, at]),
  );
  const soonest = (held: Definition[]) =>
    Math.min(...held.map((one) => reading.get(one.id) ?? Infinity));

  const byPlace = collect(nodes, (node) => node.file);
  /* A file whose only changed definition is its own module still needs a box: the module is
   * drawn as its file, so without one there's nothing on the page to stand for it — nothing
   * to light up when it's being read, and nothing for the arrow to point at. */
  for (const place of modules.keys())
    if (!byPlace.has(place)) byPlace.set(place, []);

  const groupOf = (place: string) => {
    const [first] = byPlace.get(place) ?? [];
    return ((first ?? modules.get(place))?.group ?? []).join("/");
  };
  const byGroup = collect([...byPlace.keys()], groupOf);
  const laning = stacking(byPlace, review.edges);

  const boxes = [...byGroup].map(([groupPath, everything]) => {
    /* A group is a module when one of its own modules sits at the top of it. Drawing that
     * module a box inside the box would be saying the same thing twice — the same reason a
     * module never takes a node inside its own file. */
    const root = rootOf(everything, byPlace, modules);
    const under = everything.filter(
      (path: string) => !root || path !== root.file,
    );
    const stacked = laning(under);

    const inner = [...under]
      .sort(
        (a, b) =>
          stacked(a) - stacked(b) ||
          soonest(byPlace.get(a) ?? []) - soonest(byPlace.get(b) ?? []),
      )
      .map((path) => nested(path, modules.get(path) || null));

    /* Boxes that hold each other up stack; boxes with nothing between them sit side by
     * side. Stacking those anyway made a group a single tall column with the page empty
     * either side of it. */
    const lanes: Box[][] = [];
    for (const box of inner) (lanes[stacked(box.key)] ||= []).push(box);

    return sized({
      key: groupPath,
      module: root,
      label: root ? root.name : groupPath || MISC,
      rows: [],
      lanes: lanes.filter(Boolean),
    });
  });

  /* A file's box, and inside it a box for every container it holds.
   *
   * What sits in what comes from the definitions themselves rather than from the file they
   * share, so an impl block is the box around its methods instead of a node whose diff is
   * an opening line and a closing brace with everything between it belonging to somebody
   * else. */
  function nested(path: string, module: Definition | null): Box {
    /* One outermost container is the file's box — a module drawn around its own file,
     * which is the ordinary case and needs no wrapper. Several, or none, and the file is
     * the box and they sit inside it. */
    const several = (roots.get(path) ?? []).length > 1;
    const here = byPlace.get(path) ?? [];
    const under = (of: Identity | null) =>
      all.filter((one) => one.file === path && (one.parent ?? null) === of);

    const box = (one: Definition | null, key: string, label: string): Box => {
      const children = one ? under(one.id) : [];
      const boxes = children
        .filter((child) => child.role === "container")
        .map((child) => box(child, `${path}#${child.id}`, child.name));
      /* Whatever this holds directly. The boxes inside were built first, so by the time
       * the file's own box asks, everything they claimed is spoken for — and whatever is
       * left had a container the page never drew, which shouldn't lose it its place. */
      const shown = here.filter((node) =>
        one ? node.parent === one.id : true,
      );
      for (const node of shown) placed.add(node.id);
      const left =
        one === module ? here.filter((node) => !placed.has(node.id)) : [];

      return sized({
        key,
        module: one,
        label,
        rows: layer([...shown, ...left], leansOn, reading),
        lanes: boxes.length ? [boxes] : [],
      });
    };

    const placed = new Set<Identity>();
    if (!several)
      return box(module, path, module ? module.name : guessed(path));

    const inside = (roots.get(path) ?? []).map((one) =>
      box(one, `${path}#${one.id}`, one.name),
    );
    const loose = here.filter((node) => !placed.has(node.id));
    return sized({
      key: path,
      module: null,
      label: guessed(path),
      rows: layer(loose, leansOn, reading),
      lanes: [inside],
    });
  }

  const at = new Map<Identity, Spot>();
  let y = MARGIN;
  let widest = 0;

  for (const band of bands(boxes, review)) {
    let x = MARGIN;
    for (const box of band) {
      place(box, x, y, at);
      x += box.w + BAND_GAP;
    }
    widest = Math.max(widest, x - BAND_GAP + MARGIN);
    y += Math.max(...band.map((box: Box) => box.h)) + BAND_GAP;
  }

  return { at, boxes, modules, w: widest, h: y - BAND_GAP + MARGIN };
}

/* The module a box answers to: the one nothing else in it encloses, whose file holds
 * nothing but the module itself. Anything less certain than that — two of them, or one with
 * definitions of its own to show — keeps a box of its own. */
function rootOf(
  paths: string[],
  byPlace: Map<string, Definition[]>,
  modules: Map<string, Definition>,
) {
  const roots = paths
    .map((path) => modules.get(path))
    .filter(
      (module) =>
        module &&
        module.scope.length === 0 &&
        !byPlace.get(module.file)?.length,
    );
  return roots.length === 1 ? roots[0]! : null;
}

/* Room for whatever a box holds — boxes in lanes, nodes in rows — and never narrower than
 * its own name. A box with a module wears its mark too, which is two more characters. */
function sized(box: Omit<Box, "boxes" | "w" | "h">): Box {
  const across = Math.max(
    box.lanes.length ? Math.max(...box.lanes.map(laneWidth)) : 0,
    box.rows.length ? Math.max(...box.rows.map(rowWidth)) : 0,
  );
  const lanesDeep = box.lanes.length
    ? box.lanes.reduce((sum, lane) => sum + laneHeight(lane), 0) +
      BOX_GAP * (box.lanes.length - 1)
    : 0;
  const rowsDeep = box.rows.length
    ? box.rows.length * NODE_H + ROW_GAP * (box.rows.length - 1)
    : 0;
  const between = box.lanes.length && box.rows.length ? BOX_GAP : 0;

  /* A box with nothing in it is its own name and no more. The padding above content and
   * the padding below it are both for content, and taking them anyway leaves a box with a
   * label sitting in the top of a space nothing fills. */
  const hollow = !box.lanes.length && !box.rows.length;

  return {
    ...box,
    boxes: box.lanes.flat(),
    w: Math.max(
      across + 2 * PAD_X,
      labelWidth(box.module ? `${box.label}xx` : box.label),
    ),
    h: hollow ? LABEL : PAD_TOP + lanesDeep + between + rowsDeep + PAD_BOTTOM,
  };
}

/* Placing a box is placing what it holds, which is boxes and nodes, which is the same job
 * one level in. */
function place(box: Box, x: number, y: number, at: Map<Identity, Spot>) {
  box.x = x;
  box.y = y;
  let down = y + PAD_TOP;

  for (const lane of box.lanes) {
    let across = x + PAD_X;
    for (const child of lane) {
      place(child, across, down, at);
      across += child.w + BOX_GAP;
    }
    down += laneHeight(lane) + BOX_GAP;
  }

  for (const row of box.rows) {
    let across = x + PAD_X + (box.w - 2 * PAD_X - rowWidth(row)) / 2;
    for (const node of row) {
      at.set(node.id, { x: across, y: down, w: widthOf(node.name) });
      across += widthOf(node.name) + NODE_GAP;
    }
    down += NODE_H + ROW_GAP;
  }
}

/** Every node a box holds, however deep. */
export function* inside(box: Box): Generator<Definition> {
  for (const row of box.rows) yield* row;
  for (const child of box.boxes) yield* inside(child);
}

/* Rows within a box: something sits below everything it leans on. */
function layer(
  nodes: Definition[],
  leansOn: Map<Identity, Identity[]>,
  reading: Map<Identity, number>,
) {
  const here = new Set(nodes.map((n) => n.id));
  const rows: Definition[][] = [];
  for (const node of nodes) {
    const row = depth(node.id, here, leansOn, new Map());
    (rows[row] ||= []).push(node);
  }
  /* Nothing in a row leans on anything else in it, so their order is free — and free means
   * it should go to the reading rather than to how loud each one is. Sorting by loudness
   * put the noisiest first and sent the reader back and forth across a file they were
   * being walked through in order. Anything with no place in the reading is drawn but
   * never stopped at, so it goes last and out of the way. */
  const at = (one: Definition) => reading.get(one.id) ?? Infinity;
  for (const row of rows)
    if (row) row.sort((a, b) => at(a) - at(b) || byLoudness(a, b));
  /* Row nought is whatever leans on nothing, and it goes at the top: a reader meets what
   * holds things up before the things it holds. */
  return rows.filter(Boolean).flatMap(folded);
}

/* A row of peers folded into a block about as wide as it is tall, so a file with a dozen
 * tests in it grows downwards instead of off the side of the page. Nothing in a row leans
 * on anything else in it — an edge would have put one of them a row lower — so they can be
 * split across lines without a line ever pointing the wrong way. */
function folded(row: Definition[]) {
  const across = Math.ceil(Math.sqrt(row.length));
  const lines = [];
  for (let at = 0; at < row.length; at += across)
    lines.push(row.slice(at, at + across));
  return lines;
}

/* Files stack in a group the same way nodes stack in a file and groups stack on the page:
 * whatever holds another file up sits above it. Without this the files in a group land in
 * whatever order they turned up in, and half the lines between them run the wrong way. */
function stacking(byPlace: Map<string, Definition[]>, edges: Edge[]) {
  const placeOf = new Map<Identity, string>();
  for (const [path, held] of byPlace)
    for (const node of held) placeOf.set(node.id, path);

  const leansOn = new Map<string, string[]>(
    [...byPlace.keys()].map((path) => [path, []]),
  );
  for (const edge of edges) {
    const from = placeOf.get(edge.from);
    const to = placeOf.get(edge.to);
    if (from && to && from !== to) leansOn.get(from)?.push(to);
  }

  /* Depth is worked out one group at a time, counting only what that group holds. A file
   * leaning on something in another group says nothing about where it belongs in this one —
   * which lane it lands in is a question about its neighbours — and letting those outside
   * edges count pushed files below others that weren't holding them up at all. Where the
   * groups themselves go is bands()' job. */
  return (paths: string[]) => {
    const within = new Set(paths);
    const seen = new Map<string, number>();
    return (path: string) => depth(path, within, leansOn, seen);
  };
}

/* Bands of boxes: whatever holds another box up is drawn in an earlier band.
 *
 * Which band is dagger's answer, not one worked out again here. The reading order goes by
 * the same number — what leans on no other group is read before what leans on it — and a
 * page that decided for itself would agree until it didn't, at which point the reading
 * would run around a page laid out to a different plan and feel, to whoever was following
 * it, like no plan at all. */
function bands(boxes: Box[], review: Review) {
  const found: Box[][] = [];
  for (const box of boxes) {
    const row = review.bands.get(box.key) ?? 0;
    (found[row] ||= []).push(box);
  }

  /* Side by side in a band, nothing holds anything else up, so which comes first is free
   * — and goes to whichever is read first, left to right, the way the list is worked
   * through. */
  const reading = new Map(
    review.steps.map((step, at) => [step.definition, at]),
  );
  const soonest = (box: Box) =>
    Math.min(...[...inside(box)].map((one) => reading.get(one.id) ?? Infinity));
  for (const band of found)
    if (band) band.sort((a, b) => soonest(a) - soonest(b));

  return found.filter(Boolean);
}

/* How far above the bottom something sits: one more than the furthest thing it leans on.
 * A circle is settled by whoever is asked first, which is enough — being in a circle means
 * there is no right answer, only a readable one. */
function depth<K>(
  id: K,
  within: Set<K>,
  leansOn: Map<K, K[]>,
  seen: Map<K, number>,
): number {
  if (seen.has(id)) return seen.get(id)!;
  seen.set(id, 0);
  const below = (leansOn.get(id) || []).filter(
    (other) => within.has(other) && other !== id,
  );
  const found = below.length
    ? 1 + Math.max(...below.map((other) => depth(other, within, leansOn, seen)))
    : 0;
  seen.set(id, found);
  return found;
}

const byLoudness = (a: Definition, b: Definition) =>
  LOUDNESS.indexOf(a.mark) - LOUDNESS.indexOf(b.mark) ||
  a.name.localeCompare(b.name);
const rowWidth = (row: Definition[]) =>
  row.reduce((sum, n) => sum + widthOf(n.name), 0) +
  NODE_GAP * (row.length - 1);
const laneWidth = (lane: Box[]) =>
  lane.reduce((sum, f) => sum + f.w, 0) + BOX_GAP * (lane.length - 1);
const laneHeight = (lane: Box[]) => Math.max(...lane.map((f) => f.h));

function collect<T, K>(items: T[], by: (item: T) => K) {
  const out = new Map<K, T[]>();
  for (const item of items) {
    const key = by(item);
    if (!out.has(key)) out.set(key, []);
    out.get(key)!.push(item);
  }
  return out;
}
