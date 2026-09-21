/* Checks the part of the page that isn't drawing: reading a review and working out where
 * things go. Runs on a saved review, so it needs no browser, no repository and no language
 * server.
 */

import assert from "node:assert/strict";
import fs from "node:fs";
import test from "node:test";
import type { Box, Edge, Identity, Line, Raw } from "../src/dagger.ts";
import { NODE_H, digest, layout, shorten, stitch, widthOf } from "../src/review.ts";
import { phases, standing } from "../src/progress.ts";
import { compare, focused } from "../src/review.ts";

const raw: Raw = JSON.parse(fs.readFileSync(new URL("./review.json", import.meta.url), "utf8"));
const review = digest(raw);
const laid = layout(review);
const nodes = [...review.definitions.values()].filter((d) => d.kind !== "module");

/* Two different things arrive: what there is to read, and what has to be on the page for
 * the reading to make sense. Everything in the reading order is here to be drawn, and what
 * isn't in it is the modules around the rest — never an ordinary definition quietly left
 * out of the order. */
test("everything to read is on the page, and the rest is modules", () => {
  const ordered = new Set(review.steps.map((step) => step.definition));
  for (const step of ordered) assert.ok(review.definitions.has(step));

  for (const definition of review.definitions.values()) {
    if (ordered.has(definition.id)) continue;
    assert.equal(definition.kind, "module", `${definition.path} is on the page but never read`);
  }
});

test("every definition but a module gets a place", () => {
  assert.equal(laid.at.size, nodes.length);
  for (const [, spot] of laid.at) {
    for (const measure of [spot.x, spot.y, spot.w]) assert.ok(Number.isFinite(measure));
  }
});

/* A module is its file, so the file's box carries its name instead. */
test("a module has no node of its own", () => {
  for (const definition of review.definitions.values()) {
    if (definition.kind === "module") assert.ok(!laid.at.has(definition.id));
  }
  assert.ok(laid.modules.size > 0, "the sample should have at least one module");
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

/* Boxes nest to whatever depth the grouping has, so this checks the rule rather than two
 * named levels of it: nothing is ever drawn outside the box that holds it. */
test("nothing escapes the box that holds it", () => {
  type Rect = { x?: number | undefined; y?: number | undefined; w?: number | undefined; h: number };
  const within = (child: Rect, box: Box) =>
    child.x! >= box.x! && child.x! + child.w! <= box.x! + box.w &&
    child.y! >= box.y! && child.y! + child.h <= box.y! + box.h;

  const walk = (box: Box) => {
    for (const child of box.boxes) {
      assert.ok(within(child, box), `${child.key} escapes ${box.key}`);
      walk(child);
    }
    for (const row of box.rows) {
      for (const node of row) {
        const spot = laid.at.get(node.id);
        assert.ok(within({ ...spot, h: NODE_H }, box), `${node.name} escapes ${box.key}`);
      }
    }
  };

  laid.boxes.forEach(walk);
});

/* Whatever holds something up is drawn above it, and the only lines allowed to point the
 * other way are the ones where no drawing could do better: both ends are in the same cycle,
 * so one of them has to come first. Those are drawn differently on purpose. */
test("a line only runs downwards where the code is circular", () => {
  const placed = review.edges.filter((edge) => laid.at.has(edge.from) && laid.at.has(edge.to));
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

/* Which cycle each definition belongs to, if any: Tarjan, with everything else left in a
 * group of its own. */
function cycles(edges: Edge[]) {
  const leadsTo = new Map<Identity, Identity[]>();
  for (const edge of edges) leadsTo.set(edge.from, [...(leadsTo.get(edge.from) || []), edge.to]);

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

/* The one that bit us: the graph's font grew and the width it was drawn from didn't, so
 * names hung out of their boxes. These numbers mirror where Node puts its text. */
test("a name stays inside its node", () => {
  const CHAR = 13 * 0.6;
  for (const node of nodes) {
    const shown = shorten(node.name);
    const needs = 10 + CHAR + 6 + shown.length * CHAR + 10;
    assert.ok(widthOf(node.name) >= needs, `${shown} needs ${needs}, box is ${widthOf(node.name)}`);
  }
});

test("a definition in one piece reads as itself", () => {
  const whole = [...review.definitions.values()].find(
    (definition) => definition.after && Object.values(definition.after.parts).flat().length === 1,
  );
  assert.ok(whole?.after, "no definition arrives in a single piece");
  const piece = Object.values(whole.after.parts).flat()[0];
  const shown = stitch(whole.after)!.map((line) => line.text).join("\n");
  assert.equal(shown, piece.text.replace(/^\n+|\n+$/g, ""));
});

/* A module's imports aren't one stretch of the file, and a gap has to say so rather than
 * running two distant lines together. */
test("pieces that aren't next to each other are separated", () => {
  const scattered = [...review.definitions.values()].find((definition) => {
    const pieces = definition.after && Object.values(definition.after.parts).flat();
    if (!pieces || pieces.length < 2) return false;
    const inOrder = [...pieces].sort((a, b) => a.span.start - b.span.start);
    return inOrder.some((piece, index) => index > 0 && piece.span.start > inOrder[index - 1].span.end);
  });

  if (!scattered) return;
  assert.ok(stitch(scattered.after)!.some((line) => line.at === null && line.text === "…"));
});

/* A reader points at a line — "the check on line 31" — so every line that's really in the
 * file says which one, counting up as it goes. */
test("every line says where it is in the file", () => {
  for (const definition of review.definitions.values()) {
    const lines = stitch(definition.after);
    if (!lines) continue;

    const numbered = lines.filter((line) => line.at !== null);
    for (const line of numbered) assert.ok(line.at! >= 1, `${definition.path} has line ${line.at}`);

    /* Within one stretch they run consecutively; a gap is where they may jump. */
    for (let at = 1; at < lines.length; at++) {
      const before: Line = lines[at - 1];
      const now: Line = lines[at];
      if (before.at === null || now.at === null) continue;
      assert.equal(now.at, before.at + 1, `${definition.path} jumps from ${before.at} to ${now.at}`);
    }
  }
});

/* Both snapshots are read at once, so what dagger says about them arrives interleaved.
 * Nothing here may be worked out from the order the lines turn up in. */
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
  assert.equal(laying.done, true, "laying out is over once a reading has begun");

  assert.equal(older.said, "reading aaa");
  assert.deepEqual(older.through, [2, 7], "each side took the line naming it");
  assert.equal(older.done, false);

  assert.equal(newer.said, "reading bbb");
  assert.equal(newer.done, true);
  assert.equal(newer.detail, "99 definitions");
});

/* Both are under way together, so both beat. */
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

/* A definition can be a whole file, and a file can be ten thousand lines. What's near a
 * change is context; what's far from one is a haystack. */
test("a long diff is cut down to what sits near a change", () => {
  const line = (at: number, text: string) => ({ at, text });
  const lines = Array.from({ length: 500 }, (_, at) => ({
    mark: at === 250 ? ("+" as const) : (" " as const),
    line: line(at + 1, `line ${at + 1}`),
  }));

  const shown = focused(lines, 10);
  const kept = shown.filter((one) => one.line.at !== null);
  const gaps = shown.filter((one) => one.line.at === null);

  assert.equal(kept.length, 21, "the changed line and ten either side");
  assert.equal(gaps.length, 2, "one mark for each stretch stood down");
  assert.ok(
    kept.every((one) => Math.abs(one.line.at! - 251) <= 10),
    "kept something far from the change",
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

/* Short enough to show whole, and it is: no marks where nothing was left out. */
test("a short diff is left alone", () => {
  const lines = [
    { mark: " " as const, line: { at: 1, text: "one" } },
    { mark: "+" as const, line: { at: 2, text: "two" } },
  ];
  assert.deepEqual(focused(lines, 10), lines);
});

/* Whatever it decides changed, the marks have to add up: reading everything but the
 * additions gives back the older version, and everything but the removals the newer. A
 * diff that doesn't is lying about one of them. */
test("a diff rebuilds both of the sides it came from", () => {
  const lines = (texts: string[]) => texts.map((text, at) => ({ at: at + 1, text }));
  let seed = 7;
  const next = () => (seed = (seed * 1103515245 + 12345) % 2147483648) / 2147483648;

  for (let round = 0; round < 200; round++) {
    const was = Array.from({ length: Math.floor(next() * 40) }, () =>
      String.fromCharCode(97 + Math.floor(next() * 6)),
    );
    const is = was
      .filter(() => next() > 0.3)
      .flatMap((one) => (next() > 0.8 ? [String.fromCharCode(97 + Math.floor(next() * 6)), one] : [one]));

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

/* The reason the comparison was rewritten: a definition can be a whole file, and the table
 * of every line against every other took most of a second to find a handful of changes. */
test("a small change in a long file is found without weighing every line against every other", () => {
  const lines = (texts: string[]) => texts.map((text, at) => ({ at: at + 1, text }));
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
  const lines = ["one", "two", "three"].map((text, at) => ({ at: at + 1, text }));
  assert.ok(compare(lines, [...lines]).every((one) => one.mark === " "));
});

/* The case the first long-file test missed. Its six thousand lines were all distinct, so
 * every one of them was a place the two versions could be pinned together. A file of
 * repeated punctuation — which is what generated output looks like — offers no such line,
 * and the comparison used to fall back to weighing every line against every other. */
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

  assert.ok(took < 500, `took ${took.toFixed(0)}ms — the table is being built again`);
  assert.equal(shown.filter((one) => one.mark !== " ").length, 4, "both ends, nothing else");
  assert.deepEqual(
    shown.filter((one) => one.mark !== "+").map((one) => one.line.text),
    was.map((one) => one.text),
  );
  assert.deepEqual(
    shown.filter((one) => one.mark !== "−").map((one) => one.line.text),
    is.map((one) => one.text),
  );
});
