/* The scene builder, run on a saved review so no browser is needed. */

import assert from "node:assert/strict";
import fs from "node:fs";
import test from "node:test";
import type { Box, Raw } from "../src/dagger.ts";
import { digest } from "../src/digest.ts";
import { matches, scene, type Input } from "../src/graph/scene.ts";
import { layout } from "../src/layout.ts";

const raw: Raw = JSON.parse(
  fs.readFileSync(new URL("./review.json", import.meta.url), "utf8"),
);
const review = digest(raw);
const laid = layout(review);
const here = review.steps[0].definition;
const next = review.steps[1].definition;

const input = (over: Partial<Input> = {}): Input => ({
  review,
  laid,
  here,
  next,
  read: new Set(),
  over: null,
  touching: null,
  query: "",
  ...over,
});

const every = (boxes: Box[]): Box[] =>
  boxes.flatMap((box) => [box, ...every(box.boxes)]);

test("every placed node appears once, with finite coordinates", () => {
  const nodes = scene(input()).nodes;
  assert.deepEqual(
    nodes.map((node) => node.id).sort(),
    [...laid.at.keys()].sort(),
  );
  for (const node of nodes) {
    for (const measure of [node.x, node.y, node.w, node.h, node.nameX])
      assert.ok(Number.isFinite(measure), `${node.id} at ${measure}`);
    assert.ok(node.name.length > 0);
  }
});

test("every box appears once", () => {
  const boxes = scene(input()).boxes;
  assert.deepEqual(
    boxes.map((box) => box.key).sort(),
    every(laid.boxes)
      .map((box) => box.key)
      .sort(),
  );
  assert.ok(laid.boxes.every((box) => box.boxes.length));
  assert.ok(boxes.some((box) => box.depth > 0));
});

test("every edge with both ends placed appears once", () => {
  const edges = scene(input()).edges;
  const placed = review.edges.filter(
    (edge) => laid.at.has(edge.from) && laid.at.has(edge.to),
  );
  assert.equal(edges.length, placed.length);
  assert.equal(new Set(edges.map((edge) => edge.key)).size, edges.length);
  for (const edge of edges) assert.match(edge.path, /^M .* C .*$/);
});

test("the current node is here and the next is soon", () => {
  const nodes = scene(input()).nodes;
  const of = (id: string) => nodes.find((node) => node.id === id)!.classes;
  assert.ok(of(here).includes("here"));
  assert.ok(of(next).includes("soon"));
  assert.ok(!of(here).includes("dim"));
});

test("nodes away from the current one are dim, and none are without one", () => {
  const near = new Set([
    here,
    ...review.edges.flatMap((edge) =>
      edge.from === here ? [edge.to] : edge.to === here ? [edge.from] : [],
    ),
  ]);
  for (const node of scene(input()).nodes) {
    const expected = !near.has(node.id) && node.id !== next;
    assert.equal(node.classes.includes("dim"), expected, node.id);
  }
  for (const node of scene(input({ here: null, next: null })).nodes)
    assert.ok(!node.classes.includes("dim"), node.id);
});

test("a query dims what doesn't match and nothing that does", () => {
  const query = review.definitions.get(next)!.name.slice(0, 3).toUpperCase();
  const nodes = scene(input({ query })).nodes;
  const hits = nodes.filter((node) =>
    matches(review.definitions.get(node.id)!, query),
  );
  assert.ok(hits.length > 0 && hits.length < nodes.length);
  for (const node of nodes) {
    const hit = matches(review.definitions.get(node.id)!, query);
    assert.equal(node.classes.includes("dim"), !hit, node.id);
  }
});

test("an empty query changes nothing: neighbours still dim, and the current never does", () => {
  const nodes = scene(input({ query: "" })).nodes;
  const dim = (id: string) =>
    nodes.find((node) => node.id === id)!.classes.includes("dim");
  assert.ok(!dim(here));
  assert.ok(nodes.some((node) => dim(node.id)));
  assert.equal(
    nodes.filter((node) => dim(node.id)).length,
    scene(input()).nodes.filter((node) => node.classes.includes("dim")).length,
  );
});

test("the arrow ahead points at the next step, or nowhere", () => {
  const ahead = scene(input()).ahead;
  assert.ok(ahead);
  assert.match(ahead.path, /^M .* L .*$/);
  assert.match(ahead.tip, /^M .* Z$/);
  assert.equal(scene(input({ next: null })).ahead, null);
  assert.equal(scene(input({ here: null })).ahead, null);
});

test("hovering a box lights exactly that box", () => {
  const key = every(laid.boxes).at(-1)!.key;
  const lit = scene(input({ over: key })).boxes.filter((box) => box.lit);
  assert.deepEqual(
    lit.map((box) => box.key),
    [key],
  );
  assert.ok(scene(input()).boxes.every((box) => !box.lit));
});
