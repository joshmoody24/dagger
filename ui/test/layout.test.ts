/* Runs on a saved review, so no browser, repository or language server is needed. The
 * review is pinned to one commit of this repository; regenerate it when the output shape
 * changes, from the repo root:
 *
 *   ./target/debug/dagger json commits ea9d673~1 ea9d673 \
 *     > ui/test/review.json && npm --prefix ui run format
 */

import assert from "node:assert/strict";
import fs from "node:fs";
import test from "node:test";
import type { Box, Edge, Group, Identity, Line, Raw } from "../src/dagger.ts";
import { digest } from "../src/digest.ts";
import { compare, focused, paired } from "../src/diff.ts";
import { NODE_H, layout, shorten, widthOf } from "../src/layout.ts";
import { compared, phases, standing } from "../src/progress.ts";
import { stitch } from "../src/text.ts";

const raw: Raw = JSON.parse(
  fs.readFileSync(new URL("./review.json", import.meta.url), "utf8"),
);
const review = digest(raw);
const laid = layout(review);
const nodes = [...review.definitions.values()].filter(
  (d) => d.kind !== "module",
);

/* Anything not in the reading order must be a module drawn around the rest. */
test("everything to read is on the page, and the rest is modules", () => {
  const ordered = new Set(review.steps.map((step) => step.definition));
  for (const step of ordered) assert.ok(review.definitions.has(step));

  for (const definition of review.definitions.values()) {
    if (ordered.has(definition.id)) continue;
    assert.equal(
      definition.kind,
      "module",
      `${definition.path} is on the page but never read`,
    );
  }
});

/* A module that's read is a node; one that isn't is only a box. */
test("everything to read gets a place, and nothing else", () => {
  assert.equal(laid.at.size, review.steps.length);
  for (const [, spot] of laid.at) {
    for (const measure of [spot.x, spot.y, spot.w])
      assert.ok(Number.isFinite(measure));
  }
});

test("nothing is drawn on top of anything else", () => {
  const spots = [...laid.at.values()];
  for (let i = 0; i < spots.length; i++) {
    for (let j = i + 1; j < spots.length; j++) {
      const [a, b] = [spots[i], spots[j]];
      const across = a.x < b.x + b.w && b.x < a.x + a.w;
      const down = a.y < b.y + NODE_H && b.y < a.y + NODE_H;
      assert.ok(!(across && down), `two nodes overlap at ${a.x},${a.y}`);
    }
  }
});

/* Boxes nest to any depth, so this checks the rule at every level. */
test("nothing escapes the box that holds it", () => {
  const within = (
    child: { x: number; y: number; w: number; h: number },
    box: Box,
  ) =>
    child.x >= box.x &&
    child.x + child.w <= box.x + box.w &&
    child.y >= box.y &&
    child.y + child.h <= box.y + box.h;

  const walk = (box: Box) => {
    for (const child of box.boxes) {
      assert.ok(within(child, box), `${child.key} escapes ${box.key}`);
      walk(child);
    }
    for (const id of box.nodes) {
      const spot = laid.at.get(id)!;
      assert.ok(
        within({ ...spot, h: NODE_H }, box),
        `${name(id)} escapes ${box.key}`,
      );
    }
  };

  laid.boxes.forEach(walk);
});

/* Downward edges are only allowed inside a cycle, where something has to come first. */
test("a line only runs downwards where the code is circular", () => {
  const placed = review.edges.filter(
    (edge) => laid.at.has(edge.from) && laid.at.has(edge.to),
  );
  const circular = cycles(placed);

  for (const edge of placed) {
    if (laid.at.get(edge.to)!.y <= laid.at.get(edge.from)!.y) continue;
    assert.equal(
      circular.get(edge.from),
      circular.get(edge.to),
      `${name(edge.from)} → ${name(edge.to)} runs downwards without being in a cycle`,
    );
  }
});

const name = (id: Identity) => review.definitions.get(id)!.name;

/* Tarjan's SCC: which cycle each definition belongs to. */
function cycles(edges: Edge[]) {
  const leadsTo = new Map<Identity, Identity[]>();
  for (const edge of edges)
    leadsTo.set(edge.from, [...(leadsTo.get(edge.from) || []), edge.to]);

  const order = new Map<Identity, number>();
  const low = new Map<Identity, number>();
  const open: Identity[] = [];
  const inside = new Set<Identity>();
  const group = new Map<Identity, Identity>();

  const walk = (at: Identity) => {
    order.set(at, order.size);
    low.set(at, order.get(at)!);
    open.push(at);
    inside.add(at);

    for (const next of leadsTo.get(at) || []) {
      if (!order.has(next)) walk(next);
      if (inside.has(next)) low.set(at, Math.min(low.get(at)!, low.get(next)!));
    }

    if (low.get(at) === order.get(at)) {
      while (true) {
        const off = open.pop()!;
        inside.delete(off);
        group.set(off, at);
        if (off === at) break;
      }
    }
  };

  for (const edge of edges) if (!order.has(edge.from)) walk(edge.from);
  return group;
}

/* These numbers mirror where Node puts its text. */
test("a name stays inside its node", () => {
  const CHAR = 13 * 0.6;
  for (const node of nodes) {
    const shown = shorten(node.name);
    const needs = 10 + CHAR + 6 + shown.length * CHAR + 10;
    assert.ok(
      widthOf(node.name) >= needs,
      `${shown} needs ${needs}, box is ${widthOf(node.name)}`,
    );
  }
});

test("a definition in one piece reads as itself", () => {
  const whole = [...review.definitions.values()].find(
    (definition) =>
      definition.after &&
      Object.values(definition.after.parts).flat().length === 1,
  );
  assert.ok(whole?.after, "no definition arrives in a single piece");
  const piece = Object.values(whole.after.parts).flat()[0];
  const shown = stitch(whole.after)!
    .map((line) => line.text)
    .join("\n");
  assert.equal(shown, piece.text.replace(/^\n+|\n+$/g, ""));
});

/* A gap between non-adjacent pieces is shown, not run together. */
test("pieces that aren't next to each other are separated", () => {
  const scattered = [...review.definitions.values()].find((definition) => {
    const pieces =
      definition.after && Object.values(definition.after.parts).flat();
    if (!pieces || pieces.length < 2) return false;
    const inOrder = [...pieces].sort((a, b) => a.span.start - b.span.start);
    return inOrder.some(
      (piece, index) =>
        index > 0 && piece.span.start > inOrder[index - 1].span.end,
    );
  });

  if (!scattered) return;
  assert.ok(
    stitch(scattered.after)!.some(
      (line) => line.at === null && line.text === "…",
    ),
  );
});

/* Readers refer to lines by number, so every real line carries one. */
test("every line says where it is in the file", () => {
  for (const definition of review.definitions.values()) {
    const lines = stitch(definition.after);
    if (!lines) continue;

    const numbered = lines.filter((line) => line.at !== null);
    for (const line of numbered)
      assert.ok(line.at! >= 1, `${definition.path} has line ${line.at}`);

    /* Within one stretch they run consecutively; a gap is where they may jump. */
    for (let at = 1; at < lines.length; at++) {
      const before: Line = lines[at - 1];
      const now: Line = lines[at];
      if (before.at === null || now.at === null) continue;
      assert.equal(
        now.at,
        before.at + 1,
        `${definition.path} jumps from ${before.at} to ${now.at}`,
      );
    }
  }
});

/* Both snapshots are read at once, so lines interleave; nothing may depend on their order. */
test("a progress report reads the same however the two readings interleave", () => {
  const said = [
    "comparing aaa to bbb",
    "12 files differ",
    "after · reading 9 files of bbb with dagger-lsp",
    "before · reading 9 files of aaa with dagger-lsp",
    "after ·   walked 1 of 4 files, opened 6",
    "before ·   walked 2 of 7 files, opened 9",
    "after ·   read 40 files, found 99 definitions",
  ];

  const [laying, older, newer] = phases(said);
  assert.equal(laying.done, true, "preparing is over once a reading has begun");

  assert.equal(older.said, "reading aaa");
  assert.deepEqual(older.through, [2, 7], "each side took the line naming it");
  assert.equal(older.done, false);

  assert.equal(newer.said, "reading bbb");
  assert.equal(newer.done, true);
  assert.equal(newer.detail, "99 definitions");
});

/* Both are under way together, so both beat. */
/* A side with two extractors says "read" twice; the circle used to fill after the first. */
test("a reading is not done until its last extractor is", () => {
  const [, , after] = phases([
    "comparing aaa to bbb",
    "after · reading 9 files of bbb with dagger-rust",
    "after ·   read 9 files, found 40 definitions",
    "after · reading 3 files of bbb with dagger-lsp",
  ]);
  assert.equal(after.done, false);
  assert.equal(after.detail, "with dagger-lsp");
});

test("every reading still going is shown as going", () => {
  const found = phases([
    "comparing aaa to bbb",
    "before · reading 9 files of aaa with dagger-lsp",
    "after · reading 9 files of bbb with dagger-lsp",
  ]);
  assert.deepEqual(
    found.map((_, at) => standing(found, at)),
    ["was", "at", "at"],
  );
});

test("a long diff is cut down to what sits near a change", () => {
  const line = (at: number, text: string) => ({ at, text });
  const lines = Array.from({ length: 500 }, (_, at) => ({
    mark: at === 250 ? ("+" as const) : (" " as const),
    line: line(at + 1, `line ${at + 1}`),
  }));

  const shown = focused(lines, 10);
  const kept = shown.filter((one) => one.line.at !== null);
  const gaps = shown.filter((one) => one.line.at === null);

  assert.equal(
    kept.length,
    22,
    "the signature, the changed line and ten either side",
  );
  assert.equal(gaps.length, 2, "one mark for each stretch stood down");
  assert.ok(
    kept.every(
      (one) => one.line.at === 1 || Math.abs(one.line.at! - 251) <= 10,
    ),
    "kept something far from the change",
  );
  assert.deepEqual(
    gaps.map((one) => one.gap),
    [
      { from: 1, to: 240 },
      { from: 261, to: 500 },
    ],
    "each gap records the lines it stands in for",
  );
});

test("a removed line and its replacement stress only the words that differ", () => {
  const shown = paired([
    { mark: "−", line: { at: 1, text: "let total = count + 1;" } },
    { mark: "+", line: { at: 1, text: "let total = count + 2;" } },
  ]);
  assert.deepEqual(shown[0].emphasis, [[20, 21]]);
  assert.deepEqual(shown[1].emphasis, [[20, 21]]);
});

test("lines that share too little are not stressed at all", () => {
  const shown = paired([
    { mark: "−", line: { at: 1, text: "return None" } },
    { mark: "+", line: { at: 1, text: "for x in xs: yield f(x)" } },
  ]);
  assert.ok(shown.every((one) => one.emphasis === undefined));
});

test("a run of removals then as many additions pairs up in order", () => {
  const shown = paired([
    { mark: " ", line: { at: 1, text: "{" } },
    { mark: "−", line: { at: 2, text: "a = one(x)" } },
    { mark: "−", line: { at: 3, text: "b = two(x)" } },
    { mark: "+", line: { at: 2, text: "a = one(y)" } },
    { mark: "+", line: { at: 3, text: "b = three(x)" } },
    { mark: " ", line: { at: 4, text: "}" } },
  ]);
  assert.deepEqual(
    shown.map((one) => one.emphasis),
    [undefined, [[8, 9]], [[4, 7]], [[8, 9]], [[4, 9]], undefined],
  );
});

/* Nothing changed, so there's nothing to sit near — but it still can't all be drawn. */
test("a diff with no changes at all keeps its beginning", () => {
  const lines = Array.from({ length: 500 }, (_, at) => ({
    mark: " " as const,
    line: { at: at + 1, text: `line ${at + 1}` },
  }));

  const shown = focused(lines, 10);
  assert.deepEqual(
    shown.filter((one) => one.line.at !== null).map((one) => one.line.at),
    Array.from({ length: 11 }, (_, at) => at + 1),
  );
});

test("a short diff is left alone", () => {
  const lines = [
    { mark: " " as const, line: { at: 1, text: "one" } },
    { mark: "+" as const, line: { at: 2, text: "two" } },
  ];
  assert.deepEqual(focused(lines, 10), lines);
});

/* Everything but the additions is the old side; everything but the removals is the new. */
test("a diff rebuilds both of the sides it came from", () => {
  const lines = (texts: string[]) =>
    texts.map((text, at) => ({ at: at + 1, text }));
  let seed = 7;
  const next = () =>
    (seed = (seed * 1103515245 + 12345) % 2147483648) / 2147483648;

  for (let round = 0; round < 200; round++) {
    const was = Array.from({ length: Math.floor(next() * 40) }, () =>
      String.fromCharCode(97 + Math.floor(next() * 6)),
    );
    const is = was
      .filter(() => next() > 0.3)
      .flatMap((one) =>
        next() > 0.8
          ? [String.fromCharCode(97 + Math.floor(next() * 6)), one]
          : [one],
      );

    const shown = compare(lines(was), lines(is));
    assert.deepEqual(
      shown.filter((one) => one.mark !== "+").map((one) => one.line.text),
      was,
      `round ${round} lost the older side`,
    );
    assert.deepEqual(
      shown.filter((one) => one.mark !== "−").map((one) => one.line.text),
      is,
      `round ${round} lost the newer side`,
    );
  }
});

test("a small change in a long file is found without weighing every line against every other", () => {
  const lines = (texts: string[]) =>
    texts.map((text, at) => ({ at: at + 1, text }));
  const was = Array.from({ length: 6000 }, (_, at) => `line ${at}`);
  const is = [...was];
  is.splice(3000, 2, "changed one", "changed two", "changed three");

  const shown = compare(lines(was), lines(is));
  const edits = shown.filter((one) => one.mark !== " ");

  assert.equal(edits.length, 5, "two lines gone, three arrived");
  assert.deepEqual(
    shown.filter((one) => one.mark !== "−").map((one) => one.line.text),
    is,
  );
});

test("nothing changed means nothing marked", () => {
  const lines = ["one", "two", "three"].map((text, at) => ({
    at: at + 1,
    text,
  }));
  assert.ok(compare(lines, [...lines]).every((one) => one.mark === " "));
});

/* No line is unique to both sides, so there is nothing to anchor on. */
test("a long file of repeated lines is compared without weighing every pair", () => {
  const body = Array.from({ length: 4000 }, (_, at) => ({
    at: at + 2,
    text: at % 3 === 0 ? "{" : at % 3 === 1 ? '  "via": [],' : "},",
  }));
  const was = [{ at: 1, text: "first" }, ...body, { at: 4002, text: "last" }];
  const is = [{ at: 1, text: "FIRST" }, ...body, { at: 4002, text: "LAST" }];

  const began = performance.now();
  const shown = compare(was, is);
  const took = performance.now() - began;

  assert.ok(
    took < 500,
    `took ${took.toFixed(0)}ms — the table is being built again`,
  );
  assert.equal(
    shown.filter((one) => one.mark !== " ").length,
    4,
    "both ends, nothing else",
  );
  assert.deepEqual(
    shown.filter((one) => one.mark !== "+").map((one) => one.line.text),
    was.map((one) => one.text),
  );
  assert.deepEqual(
    shown.filter((one) => one.mark !== "−").map((one) => one.line.text),
    is.map((one) => one.text),
  );
});

/* Layouts from hand-written trees, for shapes the saved review doesn't have. */
const node = (id: string, tier: number): Group => ({ type: "node", id, tier });
const box = (name: string, tier: number, children: Group[]): Group => ({
  type: "group",
  name,
  tier,
  children,
});
const paged = (groups: Group[], read: string[], hidden: string[] = []) => {
  const ids = new Set<string>();
  const walk = (group: Group) =>
    group.type === "node" ? ids.add(group.id) : group.children.forEach(walk);
  groups.forEach(walk);
  for (const id of hidden) ids.delete(id);
  const shown = {
    locator: { scope: [], name: "" },
    role: "item",
    parent: null,
    kind: "function",
    file: "one.rs",
    parts: {},
    type_from_compiler: null,
  };
  return layout({
    title: null,
    definitions: new Map(
      [...ids].map((id) => [
        id,
        {
          id,
          name: `f${id}`,
          scope: [],
          path: `f${id}`,
          file: "one.rs",
          kind: "function",
          change: "added",
          before: null,
          after: shown,
          mark: "added",
          reached: 0,
          parent: null,
        },
      ]),
    ),
    steps: read.map((definition) => ({ definition, on_faith: [] })),
    edges: [],
    ripples: 1,
    cost: { peak_open: 0, total_open: 0, taken_on_faith: 0, jumps: 0 },
    groups,
    warnings: [],
  } as never);
};

/* Where a box sits, found by its label. */
const boxAt = (laid: ReturnType<typeof layout>, label: string) => {
  let found: Box | undefined;
  const walk = (box: Box) => {
    if (box.label === label) found = box;
    box.boxes.forEach(walk);
  };
  laid.boxes.forEach(walk);
  return found!;
};

test("tiers go down the page and the order along one goes across", () => {
  const laid = paged(
    [
      box("lib", 0, [
        node("1", 0),
        node("2", 0),
        box("tests", 1, [node("3", 0)]),
        node("4", 1),
      ]),
    ],
    ["1", "2", "3", "4"],
  );
  const at = (id: string) => laid.at.get(id)!;
  assert.ok(at("1").x < at("2").x, "read first, drawn first");
  assert.ok(
    at("2").y < boxAt(laid, "tests").y,
    "a box sits below the tier above it",
  );
  assert.equal(boxAt(laid, "tests").y, at("4").y, "one tier shares a row");
  assert.ok(
    boxAt(laid, "tests").x < at("4").x,
    "a box takes its place along the row",
  );
});

test("a box with nothing left in it is not drawn", () => {
  const tree = [
    box("lib", 0, [node("1", 0)]),
    box("far", 1, [node("2", 0), node("3", 0)]),
  ];
  const whole = paged(tree, ["1", "2", "3"]);
  assert.ok(boxAt(whole, "far"), "something in it, so drawn");

  const pruned = paged(tree, ["1", "2", "3"], ["2", "3"]);
  assert.ok(boxAt(pruned, "far") === undefined, "nothing left in it");
  assert.equal(pruned.at.size, 1);
});

/* The pair the page keys its read marks by comes from what the adapter said it compared. */
test("what was compared is read off the first progress line", () => {
  assert.deepEqual(compared(["comparing aaa to bbb", "3 files differ"]), [
    "aaa",
    "bbb",
  ]);
  assert.equal(compared(["3 files differ"]), null);
});
