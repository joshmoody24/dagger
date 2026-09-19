/* Checks the part of the page that isn't drawing: reading a review and working out where
 * things go. Runs on a saved review, so it needs no browser, no repository and no language
 * server.
 */

import assert from "node:assert/strict";
import fs from "node:fs";
import test from "node:test";
import { NODE_H, digest, layout, shorten, stitch, widthOf } from "../src/review.ts";

const raw = JSON.parse(fs.readFileSync(new URL("./review.json", import.meta.url), "utf8"));
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
  const within = (child, box) =>
    child.x >= box.x && child.x + child.w <= box.x + box.w &&
    child.y >= box.y && child.y + child.h <= box.y + box.h;

  const walk = (box) => {
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
    if (laid.at.get(edge.to).y <= laid.at.get(edge.from).y) continue;
    assert.equal(
      circular.get(edge.from),
      circular.get(edge.to),
      `${name(edge.from)} → ${name(edge.to)} runs downwards without being in a cycle`,
    );
  }
});

const name = (id) => review.definitions.get(id).name;

/* Which cycle each definition belongs to, if any: Tarjan, with everything else left in a
 * group of its own. */
function cycles(edges) {
  const leadsTo = new Map();
  for (const edge of edges) leadsTo.set(edge.from, [...(leadsTo.get(edge.from) || []), edge.to]);

  const order = new Map();
  const low = new Map();
  const open = [];
  const inside = new Set();
  const group = new Map();

  const walk = (at) => {
    order.set(at, order.size);
    low.set(at, order.get(at));
    open.push(at);
    inside.add(at);

    for (const next of leadsTo.get(at) || []) {
      if (!order.has(next)) walk(next);
      if (inside.has(next)) low.set(at, Math.min(low.get(at), low.get(next)));
    }

    if (low.get(at) === order.get(at)) {
      while (true) {
        const off = open.pop();
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
  const piece = Object.values(whole.after.parts).flat()[0];
  assert.equal(stitch(whole.after), piece.text.replace(/^\n+|\n+$/g, ""));
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
  assert.ok(stitch(scattered.after).includes("…"));
});
