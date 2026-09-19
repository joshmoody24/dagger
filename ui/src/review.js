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
const ROW_GAP = 22, GROUP_GAP = 30, FILE_GAP = 14;
const PAD_X = 12, PAD_TOP = 24, PAD_BOTTOM = 10, NODE_GAP = 10, MARGIN = 16;

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

/* A box wears its name along the top, so it can't be narrower than the name. Smaller than
 * the nodes' font, and it has to agree with style.css the same way. */
const BOX_FONT = 12.5;
const labelWidth = (text) => Math.ceil(text.length * BOX_FONT * 0.6) + 22;

/** A name as the graph shows it: long ones lose their tail rather than their box. */
export const shorten = (name) =>
  name.length > LONGEST ? `${name.slice(0, LONGEST - 1)}…` : name;

/* Rounded up, never down: a box half a pixel too small is a name poking out of it. The 34
 * is the space either side plus the mark and the gap after it. */
export const widthOf = (text) => Math.ceil(shorten(text).length * CHAR) + 34;

/* What to call a module whose own definition isn't in this review, so its name never got
 * reported. The file it lives in is the best guess left, minus the extension — which is
 * about the file on disk, not about the code. */
const guessed = (path) => path.split("/").pop().replace(/\.[^.]+$/, "");

/* ---------------- what dagger said, in the shapes a page wants ---------------- */

export function digest(raw) {
  const definitions = new Map();
  for (const definition of raw.definitions) {
    const sides = definition.sides;
    const before = sides.kept ? sides.kept.before : sides.removed || null;
    const after = sides.kept ? sides.kept.after : sides.added || null;
    const shown = after || before;

    definitions.set(definition.identity, {
      id: definition.identity,
      name: shown.locator.name,
      path: [...shown.locator.scope, shown.locator.name].join("::"),
      file: shown.file,
      kind: shown.kind,
      before,
      after,
      mark: marking(raw.review, definition.identity),
      group: (raw.grouping.of || {})[definition.identity] || [],
    });
  }

  return {
    definitions,
    steps: raw.ordering.steps.filter((step) => definitions.has(step.definition)),
    edges: raw.review.edges.filter((e) => definitions.has(e.from) && definitions.has(e.to)),
    changes: raw.review.changes,
    cost: raw.ordering.cost,
    grouping: raw.grouping.name,
    worries: [
      ...raw.review.diagnostics.map(told),
      ...raw.notes.map((note) => (note.file ? `${note.file}: ${note.message}` : note.message)),
    ],
  };
}

/* One word for what happened, which is all a node has room for. */
function marking(review, id) {
  const change = review.changes[id];
  if (change === "added") return "added";
  if (change === "removed") return "removed";

  const edits = change.kept;
  if (edits.contract) return "contract";
  if (edits.parts.includes("body") || edits.parts.includes("type")) return "body";
  if (edits.parts.includes("docs")) return "docs";
  return review.affected.includes(id) ? "affected" : "still";
}

/** Whether a change to this one means its callers have to change too. */
export function broke(review, id) {
  const change = review.changes[id];
  if (change === "removed") return true;
  if (change === "added") return false;
  return Boolean(change.kept && change.kept.contract);
}

/* Dagger's diagnostics say what it had to work around. A reader deserves them in words
 * rather than as a shape of JSON. */
function told(diagnostic) {
  const [[kind, what]] = Object.entries(diagnostic);
  switch (kind) {
    case "unattributed":
      return `${what.file}: ${what.lines} changed line${what.lines === 1 ? "" : "s"} belong to no definition, around line ${what.at.join(", ")}`;
    case "lopsided_contract":
      return "one side of a definition had a compiler's word on it and the other didn't, so the signature as written was compared instead";
    case "unbound_in_contract":
      return `${what.symbol} appears where callers can see it, but nothing could say what it refers to`;
    case "mention_from_nowhere":
      return `a mention came from ${what.from.name}, which was never reported`;
    default:
      return kind;
  }
}

/* ---------------- laying it out ---------------- */

/* Boxes inside boxes: a group holds files, a file holds definitions. Whatever holds
 * something else up is drawn above it, so the page reads downwards the way the review does.
 *
 * A module is its file, so it gives the file's box its name rather than taking a node inside
 * the box that stands for the box.
 */
export function layout(review) {
  const nodes = [...review.definitions.values()].filter((d) => d.kind !== "module");
  const modules = new Map();
  for (const definition of review.definitions.values()) {
    if (definition.kind === "module") modules.set(definition.file, definition);
  }

  const leansOn = new Map(nodes.map((n) => [n.id, []]));
  for (const edge of review.edges) {
    if (leansOn.has(edge.from) && leansOn.has(edge.to)) leansOn.get(edge.from).push(edge.to);
  }

  const byFile = collect(nodes, (node) => node.file);
  const byGroup = collect([...byFile.keys()], (file) => byFile.get(file)[0].group.join("/"));
  const laning = stacking(byFile, review.edges);

  const boxes = [...byGroup].map(([groupPath, filePaths]) => {
    const stacked = laning(filePaths);
    const files = [...filePaths].sort((a, b) => stacked(a) - stacked(b)).map((path) => {
      const rows = layer(byFile.get(path), leansOn);
      const inner = Math.max(...rows.map(rowWidth));
      const module = modules.get(path) || null;
      const label = module ? module.name : guessed(path);
      return {
        key: path,
        module,
        label,
        rows,
        /* A module's box also carries its mark, so there are two more characters to fit. */
        w: Math.max(inner + 2 * PAD_X, labelWidth(module ? `${label}xx` : label)),
        h: rows.length * NODE_H + (rows.length - 1) * ROW_GAP + PAD_TOP + PAD_BOTTOM,
      };
    });

    /* Files that hold each other up stack; files that have nothing to do with each other
     * sit side by side. Stacking those anyway is what made a group a single tall column
     * with the page empty either side of it. */
    const lanes = [];
    for (const file of files) (lanes[stacked(file.key)] ||= []).push(file);
    const inLanes = lanes.filter(Boolean);

    return {
      key: groupPath,
      label: groupPath || "elsewhere",
      files,
      lanes: inLanes,
      w: Math.max(
        Math.max(...inLanes.map(laneWidth)) + 2 * PAD_X,
        labelWidth(groupPath || "elsewhere"),
      ),
      h:
        inLanes.reduce((sum, lane) => sum + laneHeight(lane), 0) +
        FILE_GAP * (inLanes.length - 1) +
        PAD_TOP +
        PAD_BOTTOM,
    };
  });

  const at = new Map();
  let y = MARGIN;
  let widest = 0;

  for (const band of bands(boxes, review)) {
    let x = MARGIN;
    for (const box of band) {
      box.x = x;
      box.y = y;
      let fileY = y + PAD_TOP;

      for (const lane of box.lanes) {
        let fileX = x + PAD_X;

        for (const file of lane) {
          file.x = fileX;
          file.y = fileY;
          let rowY = fileY + PAD_TOP;

          for (const row of file.rows) {
            let nodeX = file.x + PAD_X + (file.w - 2 * PAD_X - rowWidth(row)) / 2;
            for (const node of row) {
              at.set(node.id, { x: nodeX, y: rowY, w: widthOf(node.name) });
              nodeX += widthOf(node.name) + NODE_GAP;
            }
            rowY += NODE_H + ROW_GAP;
          }
          fileX += file.w + FILE_GAP;
        }
        fileY += laneHeight(lane) + FILE_GAP;
      }
      x += box.w + GROUP_GAP;
    }
    widest = Math.max(widest, x - GROUP_GAP + MARGIN);
    y += Math.max(...band.map((box) => box.h)) + GROUP_GAP;
  }

  return { at, boxes, modules, w: widest, h: y - GROUP_GAP + MARGIN };
}

/* Rows within a box: something sits below everything it leans on. */
function layer(nodes, leansOn) {
  const here = new Set(nodes.map((n) => n.id));
  const rows = [];
  for (const node of nodes) {
    const row = depth(node.id, here, leansOn, new Map());
    (rows[row] ||= []).push(node);
  }
  for (const row of rows) if (row) row.sort(byLoudness);
  /* Row nought is whatever leans on nothing, and it goes at the top: a reader meets what
   * holds things up before the things it holds. */
  return rows.filter(Boolean).flatMap(folded);
}

/* A row of peers folded into a block about as wide as it is tall, so a file with a dozen
 * tests in it grows downwards instead of off the side of the page. Nothing in a row leans
 * on anything else in it — an edge would have put one of them a row lower — so they can be
 * split across lines without a line ever pointing the wrong way. */
function folded(row) {
  const across = Math.ceil(Math.sqrt(row.length));
  const lines = [];
  for (let at = 0; at < row.length; at += across) lines.push(row.slice(at, at + across));
  return lines;
}

/* Files stack in a group the same way nodes stack in a file and groups stack on the page:
 * whatever holds another file up sits above it. Without this the files in a group land in
 * whatever order they turned up in, and half the lines between them run the wrong way. */
function stacking(byFile, edges) {
  const fileOf = new Map();
  for (const [path, inside] of byFile) for (const node of inside) fileOf.set(node.id, path);

  const leansOn = new Map([...byFile.keys()].map((path) => [path, []]));
  for (const edge of edges) {
    const from = fileOf.get(edge.from);
    const to = fileOf.get(edge.to);
    if (from && to && from !== to) leansOn.get(from).push(to);
  }

  /* Depth is worked out one group at a time, counting only what that group holds. A file
   * leaning on something in another group says nothing about where it belongs in this one —
   * which lane it lands in is a question about its neighbours — and letting those outside
   * edges count pushed files below others that weren't holding them up at all. Where the
   * groups themselves go is bands()' job. */
  return (paths) => {
    const within = new Set(paths);
    const seen = new Map();
    return (path) => depth(path, within, leansOn, seen);
  };
}

/* Bands of boxes: whatever holds another box up is drawn in an earlier band. */
function bands(boxes, review) {
  const groupOf = new Map();
  for (const box of boxes) {
    for (const file of box.files) {
      for (const row of file.rows) for (const node of row) groupOf.set(node.id, box.key);
    }
  }

  const leansOn = new Map(boxes.map((box) => [box.key, []]));
  for (const edge of review.edges) {
    const from = groupOf.get(edge.from);
    const to = groupOf.get(edge.to);
    if (from && to && from !== to) leansOn.get(from).push(to);
  }

  const keys = new Set(boxes.map((box) => box.key));
  const found = [];
  for (const box of boxes) {
    const row = depth(box.key, keys, leansOn, new Map());
    (found[row] ||= []).push(box);
  }
  return found.filter(Boolean);
}

/* How far above the bottom something sits: one more than the furthest thing it leans on.
 * A circle is settled by whoever is asked first, which is enough — being in a circle means
 * there is no right answer, only a readable one. */
function depth(id, within, leansOn, seen) {
  if (seen.has(id)) return seen.get(id);
  seen.set(id, 0);
  const below = (leansOn.get(id) || []).filter((other) => within.has(other) && other !== id);
  const found = below.length
    ? 1 + Math.max(...below.map((other) => depth(other, within, leansOn, seen)))
    : 0;
  seen.set(id, found);
  return found;
}

const byLoudness = (a, b) =>
  LOUDNESS.indexOf(a.mark) - LOUDNESS.indexOf(b.mark) || a.name.localeCompare(b.name);
const rowWidth = (row) => row.reduce((sum, n) => sum + widthOf(n.name), 0) + NODE_GAP * (row.length - 1);
const laneWidth = (lane) => lane.reduce((sum, f) => sum + f.w, 0) + FILE_GAP * (lane.length - 1);
const laneHeight = (lane) => Math.max(...lane.map((f) => f.h));

function collect(items, by) {
  const out = new Map();
  for (const item of items) {
    const key = by(item);
    if (!out.has(key)) out.set(key, []);
    out.get(key).push(item);
  }
  return out;
}

/* ---------------- the text of a definition ---------------- */

/* One diff, not one per part. The split into parts decides what breaks callers; it isn't how
 * anybody reads code. Where a part's pieces aren't next to each other in the file — a
 * module's imports, an implementation's braces — a gap stands in rather than pretending the
 * lines met. */
export function stitch(occurrence) {
  if (!occurrence) return null;

  const pieces = Object.values(occurrence.parts)
    .flat()
    .sort((a, b) => a.span.start - b.span.start);

  let out = "";
  let last = null;
  for (const piece of pieces) {
    /* Pieces that touch are run together exactly as the file has them. Putting a newline
     * between them instead is how a signature and its opening brace ended up on separate
     * lines. Only a real gap gets a line of its own. */
    if (last !== null && piece.span.start > last) out += "\n…\n";
    out += piece.text;
    last = piece.span.end;
  }

  return straighten(out.replace(/^\n+|\n+$/g, ""));
}

/* A definition starts at its name rather than at the margin, so its first line turns up
 * without the indentation every line beneath it still carries. Taking that much off the
 * rest lines them up the way the file has them. */
function straighten(text) {
  const lines = text.split("\n");
  const under = lines.slice(1).filter((line) => line.trim() && line !== "…");
  if (!under.length) return text;

  const spare = Math.min(...under.map((line) => line.match(/^ */)[0].length));
  if (!spare) return text;

  return [lines[0], ...lines.slice(1).map((line) => line.slice(spare))].join("\n");
}

/* Names in this review that can be pointed at without ambiguity.
 *
 * What a highlighter can't know: which words in this code are things the reader is about to
 * read, or has just read. A name appearing twice is left alone — pointing at the wrong one
 * is worse than pointing at nothing. */
export function namesIn(review) {
  const seen = new Map();
  for (const definition of review.definitions.values()) {
    seen.set(definition.name, seen.has(definition.name) ? null : definition.id);
  }
  for (const [name, id] of seen) if (id === null) seen.delete(name);
  return seen;
}

const TOKENS = /(\/\/[^\n]*|#[^\n]*|\/\*[\s\S]*?(?:\*\/|$))|("(?:[^"\\]|\\.)*"?|'(?:[^'\\]|\\.)*'?|`(?:[^`\\]|\\.)*`?)|([A-Za-z_$][\w$]*)|([\s\S])/g;

/** One line of code, split into what it's made of. */
export function tokens(line) {
  const out = [];
  let match;
  TOKENS.lastIndex = 0;

  while ((match = TOKENS.exec(line))) {
    const [, comment, string, name, other] = match;
    const kind = comment ? "quiet" : string ? "quiet" : name ? "name" : "plain";
    const text = comment || string || name || other;

    const last = out[out.length - 1];
    if (last && last.kind === kind && kind === "plain") last.text += text;
    else out.push({ kind, text });
  }
  return out;
}

/** Line by line, marked as kept, gone, or new. */
export function compare(before, after) {
  const a = before === null ? [] : before.split("\n");
  const b = after === null ? [] : after.split("\n");
  const same = Array.from({ length: a.length + 1 }, () => new Array(b.length + 1).fill(0));

  for (let i = a.length - 1; i >= 0; i--) {
    for (let j = b.length - 1; j >= 0; j--) {
      same[i][j] = a[i] === b[j] ? same[i + 1][j + 1] + 1 : Math.max(same[i + 1][j], same[i][j + 1]);
    }
  }

  const out = [];
  let i = 0;
  let j = 0;
  while (i < a.length && j < b.length) {
    if (a[i] === b[j]) { out.push([" ", a[i]]); i++; j++; }
    else if (same[i + 1][j] >= same[i][j + 1]) out.push(["−", a[i++]]);
    else out.push(["+", b[j++]]);
  }
  while (i < a.length) out.push(["−", a[i++]]);
  while (j < b.length) out.push(["+", b[j++]]);
  return out;
}
