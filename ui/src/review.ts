import type {
  Box,
  Definition,
  Diagnostic,
  Edge,
  Identity,
  Laid,
  Occurrence,
  Raw,
  Review,
  Line,
  Shown,
  Spot,
  Worry,
} from "./dagger.ts";

/* Reads what dagger says about a change and works out where to draw it.
 *
 * Nothing here decides anything about the code. Which definitions are worth reading, what
 * order to read them in, what broke what — all of that arrives already settled, and this
 * only places it on a page. Where the mock computed a reading order in the browser, it now
 * comes from the review itself.
 *
 * Plain functions, no components: this is the part worth testing without a browser.
 */

export const NODE_H = 28;
const ROW_GAP = 22, BAND_GAP = 30, BOX_GAP = 14;
const PAD_X = 12, PAD_TOP = 24, PAD_BOTTOM = 10, NODE_GAP = 10, MARGIN = 16;
/* Room for a box's name and nothing else, which is all an empty one needs. */
const LABEL = 26;

export const MARK = {
  added: "+", removed: "−", contract: "!", body: "~", docs: '"', affected: "≈", still: "·",
};
export const TINT = {
  added: "add", removed: "del", contract: "chg", body: "chg", docs: "chg", affected: "aff", still: "aff",
};
/* Louder first, so a row of nodes reads worst-first when nothing else decides the order. */
const LOUDNESS = ["contract", "removed", "added", "body", "docs", "affected", "still"];

/* How wide a character is in the graph's font, which has to agree with style.css: a
 * monospace advance is 0.6 of its size, and the boxes are drawn from this rather than
 * measured, since there's no page to measure against when this is tested. */
const FONT = 14;
const CHAR = FONT * 0.6;
/* Past this a name is cut short. A test is a sentence with underscores in it, and one of
 * them is as wide as a dozen ordinary definitions put together — which buries them. */
const LONGEST = 22;

/* Kept in one place so a node and the boxes around it can't drift apart. Concentric
 * corners would want each radius to be the one inside it plus the padding, which on boxes
 * this big comes out far too round — these stay tight and merely step up. */
/* Kept in one place so a node and the boxes around it can't drift apart. Each box inside
 * another is a touch tighter, down to the radius a node has — concentric corners would want
 * the padding added at every level, which on boxes this big comes out far too round. */
export const RADIUS = { node: 5, box: 9, step: 2 };

/* A box wears its name along the top, so it can't be narrower than the name. Smaller than
 * the nodes' font, and it has to agree with style.css the same way. */
const BOX_FONT = 12.5;
const labelWidth = (text: string) => Math.ceil(text.length * BOX_FONT * 0.6) + 22;

/** A name as the graph shows it: long ones lose their tail rather than their box. */
export const shorten = (name: string) =>
  name.length > LONGEST ? `${name.slice(0, LONGEST - 1)}…` : name;

/* Rounded up, never down: a box half a pixel too small is a name poking out of it. The 34
 * is the space either side plus the mark and the gap after it. */
export const widthOf = (text: string) => Math.ceil(shorten(text).length * CHAR) + 34;

/* Everything with no package above it: a script at the top of the repository, a config
 * file. They have nothing to do with each other, which is the point of keeping them in one
 * place — a reader already looking at odds and ends may as well see the rest of them than
 * keep coming back between packages. */
const MISC = "misc";

/* What to call a module whose own definition isn't in this review, so its name never got
 * reported. The file it lives in is the best guess left, minus the extension — which is
 * about the file on disk, not about the code. */
const guessed = (path: string) => (path.split("/").at(-1) ?? path).replace(/\.[^.]+$/, "");

/* ---------------- what dagger said, in the shapes a page wants ---------------- */

export function digest(raw: Raw): Review {
  const definitions = new Map<Identity, Definition>();
  for (const definition of raw.definitions) {
    /* Which sides there are is the one thing the model won't let you get wrong: a
     * definition is added, removed, or kept with both — never neither. Asked this way
     * rather than by reaching for a field, the compiler holds that to it. */
    const sides = definition.sides;
    const before = "kept" in sides ? sides.kept.before : "removed" in sides ? sides.removed : null;
    const after = "kept" in sides ? sides.kept.after : "added" in sides ? sides.added : null;
    const shown = (after || before)!;

    definitions.set(definition.identity, {
      id: definition.identity,
      name: shown.locator.name,
      scope: shown.locator.scope,
      path: [...shown.locator.scope, shown.locator.name].join("::"),
      file: shown.file,
      kind: shown.kind,
      before,
      after,
      mark: marking(raw.review, definition.identity),
      away: raw.review.affected[definition.identity] ?? 0,
      group: (raw.grouping.of || {})[definition.identity] || [],
    });
  }

  return {
    definitions,
    steps: raw.ordering.steps.filter((step) => definitions.has(step.definition)),
    edges: raw.review.edges.filter((e) => definitions.has(e.from) && definitions.has(e.to)),
    changes: raw.review.changes,
    /* What the change reached, and how far out each one sits. Kept apart so a reader can
     * turn it down: a change to something everything leans on reaches hundreds of these,
     * and the ones furthest out are the same news arriving again. */
    affected: new Map(Object.entries(raw.review.affected).map(([id, away]) => [Number(id), away])),
    ripples: raw.review.ripples,
    cost: raw.ordering.cost,
    grouping: raw.grouping.name,
    bands: new Map(Object.entries(raw.grouping.bands ?? {})),
    worries: [
      ...raw.review.diagnostics.map((diagnostic) => told(diagnostic, definitions)),
      /* An adapter's note is always about something it couldn't do, so whatever it was
       * about isn't in the review. */
      ...raw.notes.map((note) => ({
        said: note.file ? `${note.file}: ${note.message}` : note.message,
        hides: true,
      })),
    ],
  };
}

/* One word for what happened, which is all a node has room for. */
function marking(review: Raw["review"], id: Identity) {
  const change = review.changes[id];
  if (change === "added") return "added";
  if (change === "removed") return "removed";

  const edits = change.kept;
  if (edits.contract) return "contract";
  if (edits.parts.includes("body") || edits.parts.includes("type")) return "body";
  if (edits.parts.includes("docs")) return "docs";
  return review.affected[id] !== undefined ? "affected" : "still";
}

/** Whether a change to this one means its callers have to change too. */
export function broke(review: Review, id: Identity) {
  const change = review.changes[id];
  if (change === "removed") return true;
  if (change === "added") return false;
  return Boolean(change.kept && change.kept.contract);
}

/* Dagger's diagnostics say what it had to work around. A reader deserves them in words
 * rather than as a shape of JSON. */
function told(diagnostic: Diagnostic, definitions: Map<Identity, Definition>): Worry {
  const [[kind, what]] = Object.entries(diagnostic);
  /* Every one of these is about a particular definition, and five copies of the same
   * sentence with nothing to tell them apart is no use to anybody. */
  const named = (id: Identity) => {
    const definition = definitions.get(id);
    return definition ? `${definition.path} (${definition.file})` : `definition ${id}`;
  };

  /* Whether the review might not be showing something that changed, which is a different
   * kind of news from having worked something out a weaker way. Said together, the one
   * that matters is lost among the ones that don't. */
  switch (kind) {
    case "unattributed":
      return {
        hides: true,
        said: `${what.file}: ${what.lines} changed line${what.lines === 1 ? "" : "s"} belong to no definition, around line ${what.at.join(", ")}`,
      };
    case "unbound_in_contract":
      return {
        hides: true,
        said: `${what.symbol} appears where callers of ${named(what.definition)} can see it, but nothing could say what it refers to`,
      };
    case "mention_from_nowhere":
      return {
        hides: true,
        said: `a mention of ${named(what.to)} came from ${what.from.name}, which was never reported`,
      };
    case "tangled":
      return {
        hides: true,
        said: `${[...what.definition.scope, what.definition.name].join("::")} was handed over with two of its pieces covering the same text, around byte ${what.at} — so a line of it is shown twice, and read as two different kinds of change`,
      };
    case "two_of_one_name":
      return {
        hides: true,
        said: `${what.times} definitions are called ${[...what.locator.scope, what.locator.name].join("::")}, so only one of them could be followed from one side to the other`,
      };
    case "lopsided_contract":
      return {
        hides: false,
        said: `${named(what.definition)}: the compiler described one side of this and not the other, so the signature as written was compared instead`,
      };
    default:
      return { hides: true, said: `${kind}: ${JSON.stringify(what)}` };
  }
}

/* ---------------- laying it out ---------------- */

/* Boxes inside boxes, all the way down.
 *
 * A box is a place: it holds definitions of its own, and it holds other boxes. A group and
 * a file are the same thing at different depths, so nothing below tells them apart — a box
 * is sized, placed, lit and pressed the same way wherever it sits. Whatever holds something
 * else up is drawn above it, so the page reads downwards the way the review does.
 *
 * A module is its place, so it gives its box a name rather than taking a node inside the
 * box that stands for the box.
 */
export function layout(review: Review): Laid {
  const nodes = [...review.definitions.values()].filter((d) => d.kind !== "module");
  const modules = new Map<string, Definition>();
  for (const definition of review.definitions.values()) {
    if (definition.kind === "module") modules.set(definition.file, definition);
  }

  const leansOn = new Map<Identity, Identity[]>(nodes.map((n) => [n.id, []]));
  for (const edge of review.edges) {
    if (leansOn.has(edge.to)) leansOn.get(edge.from)?.push(edge.to);
  }

  /* Where each definition falls in the reading. The page can't always put things in that
   * order — what holds something up is drawn above it, and that's what the lines mean —
   * but wherever the shape leaves a choice, the choice goes to the reading. A reader
   * working down the list shouldn't have their eye thrown across the page and back. */
  const reading = new Map(review.steps.map((step, at) => [step.definition, at]));
  const soonest = (held: Definition[]) =>
    Math.min(...held.map((one) => reading.get(one.id) ?? Infinity));

  const byPlace = collect(nodes, (node) => node.file);
  /* A file whose only changed definition is its own module still needs a box: the module is
   * drawn as its file, so without one there's nothing on the page to stand for it — nothing
   * to light up when it's being read, and nothing for the arrow to point at. */
  for (const place of modules.keys()) if (!byPlace.has(place)) byPlace.set(place, []);

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
    const under = everything.filter((path: string) => !root || path !== root.file);
    const stacked = laning(under);

    const inner = [...under]
      .sort(
        (a, b) =>
          stacked(a) - stacked(b) ||
          soonest(byPlace.get(a) ?? []) - soonest(byPlace.get(b) ?? []),
      )
      .map((path) => {
        const module = modules.get(path) || null;
        return sized({
          key: path,
          module,
          label: module ? module.name : guessed(path),
          rows: layer(byPlace.get(path) ?? [], leansOn, reading),
          lanes: [],
        });
      });

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
function rootOf(paths: string[], byPlace: Map<string, Definition[]>, modules: Map<string, Definition>) {
  const roots = paths
    .map((path) => modules.get(path))
    .filter((module) => module && module.scope.length === 0 && !byPlace.get(module.file)?.length);
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
    ? box.lanes.reduce((sum, lane) => sum + laneHeight(lane), 0) + BOX_GAP * (box.lanes.length - 1)
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
    w: Math.max(across + 2 * PAD_X, labelWidth(box.module ? `${box.label}xx` : box.label)),
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
  for (const row of rows) if (row) row.sort((a, b) => at(a) - at(b) || byLoudness(a, b));
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
  for (let at = 0; at < row.length; at += across) lines.push(row.slice(at, at + across));
  return lines;
}

/* Files stack in a group the same way nodes stack in a file and groups stack on the page:
 * whatever holds another file up sits above it. Without this the files in a group land in
 * whatever order they turned up in, and half the lines between them run the wrong way. */
function stacking(byPlace: Map<string, Definition[]>, edges: Edge[]) {
  const placeOf = new Map<Identity, string>();
  for (const [path, held] of byPlace) for (const node of held) placeOf.set(node.id, path);

  const leansOn = new Map<string, string[]>([...byPlace.keys()].map((path) => [path, []]));
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
  const reading = new Map(review.steps.map((step, at) => [step.definition, at]));
  const soonest = (box: Box) =>
    Math.min(...[...inside(box)].map((one) => reading.get(one.id) ?? Infinity));
  for (const band of found) if (band) band.sort((a, b) => soonest(a) - soonest(b));

  return found.filter(Boolean);
}

/* How far above the bottom something sits: one more than the furthest thing it leans on.
 * A circle is settled by whoever is asked first, which is enough — being in a circle means
 * there is no right answer, only a readable one. */
function depth<K>(id: K, within: Set<K>, leansOn: Map<K, K[]>, seen: Map<K, number>): number {
  if (seen.has(id)) return seen.get(id)!;
  seen.set(id, 0);
  const below = (leansOn.get(id) || []).filter((other) => within.has(other) && other !== id);
  const found = below.length
    ? 1 + Math.max(...below.map((other) => depth(other, within, leansOn, seen)))
    : 0;
  seen.set(id, found);
  return found;
}

const byLoudness = (a: Definition, b: Definition) =>
  LOUDNESS.indexOf(a.mark) - LOUDNESS.indexOf(b.mark) || a.name.localeCompare(b.name);
const rowWidth = (row: Definition[]) =>
  row.reduce((sum, n) => sum + widthOf(n.name), 0) + NODE_GAP * (row.length - 1);
const laneWidth = (lane: Box[]) => lane.reduce((sum, f) => sum + f.w, 0) + BOX_GAP * (lane.length - 1);
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

/* ---------------- the text of a definition ---------------- */

/* One diff, not one per part. The split into parts decides what breaks callers; it isn't how
 * anybody reads code. Where a part's pieces aren't next to each other in the file — a
 * module's imports, an implementation's braces — a gap stands in rather than pretending the
 * lines met. */
export function stitch(occurrence: Occurrence | null): Line[] | null {
  if (!occurrence) return null;

  const pieces = Object.values(occurrence.parts)
    .flat()
    .sort((a, b) => a.span.start - b.span.start);

  const out: Line[] = [];
  let last: number | null = null;

  for (const piece of pieces) {
    /* Pieces that touch are run together exactly as the file has them. Putting a newline
     * between them instead is how a signature and its opening brace ended up on separate
     * lines. Only a real gap gets a line of its own — and that line is nowhere in the
     * file, so it has no number. */
    const joins = last !== null && piece.span.start === last;
    if (last !== null && !joins) out.push({ at: null, text: "…" });

    for (const [after, text] of piece.text.split("\n").entries()) {
      /* The first line of a piece carrying straight on from the last one finishes that
       * line rather than starting another. */
      if (joins && after === 0 && out.length) out[out.length - 1].text += text;
      else out.push({ at: piece.line + after, text });
    }
    last = piece.span.end;
  }

  return straighten(trimmed(out));
}

/** Blank lines at either end are the space around a definition, not part of it. */
function trimmed(lines: Line[]) {
  let from = 0;
  let until = lines.length;
  while (from < until && !lines[from].text.trim()) from += 1;
  while (until > from && !lines[until - 1].text.trim()) until -= 1;
  return lines.slice(from, until);
}

/* A definition starts at its name rather than at the margin, so its first line turns up
 * without the indentation every line beneath it still carries. Taking that much off the
 * rest lines them up the way the file has them. */
function straighten(lines: Line[]) {
  const under = lines.slice(1).filter((line) => line.text.trim() && line.at !== null);
  if (!under.length) return lines;

  const spare = Math.min(...under.map((line) => line.text.match(/^ */)![0].length));
  if (!spare) return lines;

  return lines.map((line, at) => (at ? { ...line, text: line.text.slice(spare) } : line));
}

/* Names in this review that can be pointed at without ambiguity.
 *
 * What a highlighter can't know: which words in this code are things the reader is about to
 * read, or has just read. A name appearing twice is left alone — pointing at the wrong one
 * is worse than pointing at nothing. */
export function namesIn(review: Review) {
  const seen = new Map<string, Identity | null>();
  for (const definition of review.definitions.values()) {
    seen.set(definition.name, seen.has(definition.name) ? null : definition.id);
  }
  const only = new Map<string, Identity>();
  for (const [name, id] of seen) if (id !== null) only.set(name, id);
  return only;
}

/* How much of a file to show around a change.
 *
 * A definition can be a whole file, and a file can be ten thousand lines: a module holds
 * its own prose and every import, and a generated one holds all of it. Drawing that to
 * explain a change of four lines is slow to put on the page and slower to find anything
 * in. Far enough away, unchanged code stops being context and becomes the haystack. */
const REACH = 100;

/** What's worth showing: everything near a change, and a mark where the rest was. */
export function focused(lines: Shown[], reach = REACH): Shown[] {
  const changed = lines.flatMap((one, at) => (one.mark === " " ? [] : [at]));
  /* Nothing changed at all — a definition here because something it leans on moved — so
   * there's no change to sit near. The top of it is the part worth having. */
  const anchors = changed.length ? changed : [0];

  const near = new Set<number>();
  for (const at of anchors) {
    const [from, to] = [Math.max(0, at - reach), Math.min(lines.length - 1, at + reach)];
    for (let line = from; line <= to; line++) near.add(line);
  }
  if (near.size === lines.length) return lines;

  const shown: Shown[] = [];
  let standing = false;
  for (let at = 0; at < lines.length; at++) {
    if (near.has(at)) {
      shown.push(lines[at]);
      standing = false;
    } else if (!standing) {
      shown.push({ mark: " ", line: { at: null, text: "…" } });
      standing = true;
    }
  }
  return shown;
}

/* How big a table of lines against lines is worth building. A hundred against a hundred is
 * instant; six thousand against six thousand is not. */
const EXACT = 40_000;

/** Line by line, marked as kept, gone, or new. */
export function compare(before: Line[] | null, after: Line[] | null): Shown[] {
  return diffing(before ?? [], after ?? []);
}

/* Lines against lines.
 *
 * The exact answer — the longest run of lines both sides share — costs a table of every
 * line against every other. That's fine until a definition is a whole file: six thousand
 * lines against six thousand is thirty-eight million cells, built to find sixteen changed
 * lines, and the page stops answering while it counts them.
 *
 * So the easy agreements are taken first. Matching ends line up and can't be anything else.
 * Then lines that appear exactly once on each side: a line with one home in each version
 * can only be that same line, wherever it has moved to. What's left between those is small,
 * and the exact answer is cheap on small things.
 */
function diffing(a: Line[], b: Line[]): Shown[] {
  let head = 0;
  while (head < a.length && head < b.length && a[head].text === b[head].text) head++;

  let tail = 0;
  while (
    tail < a.length - head &&
    tail < b.length - head &&
    a[a.length - 1 - tail].text === b[b.length - 1 - tail].text
  ) {
    tail++;
  }

  const kept = (lines: Line[]): Shown[] => lines.map((line) => ({ mark: " " as const, line }));
  const [x, y] = [a.slice(head, a.length - tail), b.slice(head, b.length - tail)];
  const middle = x.length * y.length <= EXACT ? exactly(x, y) : split(x, y);

  return [...kept(b.slice(0, head)), ...middle, ...kept(b.slice(b.length - tail))];
}

/* Split around the lines that can only be themselves, and work on what's between them.
 *
 * A line appearing exactly once in each version is a place the two certainly meet, whatever
 * happened around it. Taking those as fixed turns one enormous comparison into many small
 * ones — and where there are none to be had, there's nothing for it but the table.
 */
function split(a: Line[], b: Line[]): Shown[] {
  const counted = (lines: Line[]) => {
    const seen = new Map<string, number>();
    for (const line of lines) seen.set(line.text, (seen.get(line.text) ?? 0) + 1);
    return seen;
  };
  const [inA, inB] = [counted(a), counted(b)];

  const whereB = new Map<string, number>();
  b.forEach((line, at) => {
    if (inB.get(line.text) === 1) whereB.set(line.text, at);
  });

  const pairs: [number, number][] = [];
  a.forEach((line, at) => {
    const there = whereB.get(line.text);
    if (inA.get(line.text) === 1 && there !== undefined) pairs.push([at, there]);
  });

  const anchors = rising(pairs);
  if (!anchors.length) return exactly(a, b);


  const shown: Shown[] = [];
  let [i, j] = [0, 0];
  for (const [x, y] of anchors) {
    shown.push(...diffing(a.slice(i, x), b.slice(j, y)));
    shown.push({ mark: " ", line: b[y] });
    [i, j] = [x + 1, y + 1];
  }
  shown.push(...diffing(a.slice(i), b.slice(j)));
  return shown;
}

/* Lined up where they sit, when there's nothing to line them up by.
 *
 * Last resort, for a stretch too big to weigh line against line and with no line in it
 * distinctive enough to anchor on. Comparing position against position is the one thing
 * left that's honest: where two versions of a repeated structure agree at a spot they are
 * almost certainly the same line, and where they don't the reader is shown both. Not the
 * shortest answer, but a true one, and it costs a single pass.
 */
function abreast(a: Line[], b: Line[]): Shown[] {
  const shown: Shown[] = [];
  for (let at = 0; at < Math.max(a.length, b.length); at++) {
    const [was, is] = [a[at], b[at]];
    if (was && is && was.text === is.text) {
      shown.push({ mark: " ", line: is });
      continue;
    }
    /* Gone before arrived, so reading past the additions still gives back the older
     * version and reading past the removals the newer. */
    if (was) shown.push({ mark: "−", line: was });
    if (is) shown.push({ mark: "+", line: is });
  }
  return shown;
}

/* The longest run of meeting points that moves forwards on both sides.
 *
 * Lines that meet in both versions can still have swapped places, and a pair that goes
 * backwards would have the diff crossing over itself. The longest run that doesn't is the
 * most of the file that can be left alone.
 */
function rising(pairs: [number, number][]): [number, number][] {
  const ends: number[] = [];
  const before: number[] = [];

  for (let at = 0; at < pairs.length; at++) {
    const [, y] = pairs[at];
    let low = 0;
    let high = ends.length;
    while (low < high) {
      const mid = (low + high) >> 1;
      if (pairs[ends[mid]][1] < y) low = mid + 1;
      else high = mid;
    }
    before[at] = low > 0 ? ends[low - 1] : -1;
    ends[low] = at;
  }

  const found: [number, number][] = [];
  let at = ends.length ? ends[ends.length - 1] : -1;
  while (at >= 0) {
    found.push(pairs[at]);
    at = before[at]!;
  }
  return found.reverse();
}

/* Every line against every other: the exact answer, for when there's little enough left to
 * ask for it. This is what the whole comparison used to be.
 *
 * Guarded here rather than at each place it's called, so nothing can reach the table by a
 * route that forgot to check. Nothing should arrive too big — the ends are matched off
 * first and the middle split at lines that can only be themselves — but a stretch with no
 * line appearing exactly once on each side has nothing to split on, and a file of repeated
 * punctuation is exactly that. */
function exactly(a: Line[], b: Line[]): Shown[] {
  if (a.length * b.length > EXACT) return abreast(a, b);

  const same = Array.from({ length: a.length + 1 }, () => new Array<number>(b.length + 1).fill(0));

  for (let i = a.length - 1; i >= 0; i--) {
    for (let j = b.length - 1; j >= 0; j--) {
      same[i][j] =
        a[i].text === b[j].text
          ? same[i + 1][j + 1] + 1
          : Math.max(same[i + 1][j], same[i][j + 1]);
    }
  }

  const shown: Shown[] = [];
  let i = 0;
  let j = 0;
  while (i < a.length && j < b.length) {
    if (a[i].text === b[j].text) {
      shown.push({ mark: " ", line: b[j] });
      i++;
      j++;
    } else if (same[i + 1][j] >= same[i][j + 1]) {
      shown.push({ mark: "−", line: a[i++] });
    } else {
      shown.push({ mark: "+", line: b[j++] });
    }
  }
  while (i < a.length) shown.push({ mark: "−", line: a[i++] });
  while (j < b.length) shown.push({ mark: "+", line: b[j++] });
  return shown;
}
