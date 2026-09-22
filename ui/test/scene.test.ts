/* The scene builder, run on a saved review so no browser is needed. */

import assert from "node:assert/strict";
import fs from "node:fs";
import test from "node:test";
import type { Placed, Raw } from "../src/dagger.ts";
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

const every = (groups: Placed[]): Placed[] =>
  groups.flatMap((group) => [group, ...every(group.groups)]);

test("every placed definition appears once, with finite coordinates", () => {
  const definitions = scene(input()).definitions;
  assert.deepEqual(
    definitions.map((definition) => definition.id).sort(),
    [...laid.at.keys()].sort(),
  );
  for (const definition of definitions) {
    for (const measure of [
      definition.x,
      definition.y,
      definition.w,
      definition.h,
      definition.nameX,
    ])
      assert.ok(Number.isFinite(measure), `${definition.id} at ${measure}`);
    assert.ok(definition.name.length > 0);
  }
});

test("every group appears once", () => {
  const groups = scene(input()).groups;
  assert.deepEqual(
    groups.map((group) => group.key).sort(),
    every(laid.groups)
      .map((group) => group.key)
      .sort(),
  );
  assert.ok(laid.groups.every((group) => group.groups.length));
  assert.ok(groups.some((group) => group.depth > 0));
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

test("the current definition is here and the next is soon", () => {
  const definitions = scene(input()).definitions;
  const of = (id: string) =>
    definitions.find((definition) => definition.id === id)!.classes;
  assert.ok(of(here).includes("here"));
  assert.ok(of(next).includes("soon"));
  assert.ok(!of(here).includes("dim"));
});

test("definitions away from the current one are dim, and none are without one", () => {
  const near = new Set([
    here,
    ...review.edges.flatMap((edge) =>
      edge.from === here ? [edge.to] : edge.to === here ? [edge.from] : [],
    ),
  ]);
  for (const definition of scene(input()).definitions) {
    const expected = !near.has(definition.id) && definition.id !== next;
    assert.equal(definition.classes.includes("dim"), expected, definition.id);
  }
  for (const definition of scene(input({ here: null, next: null })).definitions)
    assert.ok(!definition.classes.includes("dim"), definition.id);
});

test("a query dims what doesn't match and nothing that does", () => {
  const query = review.definitions.get(next)!.name.slice(0, 3).toUpperCase();
  const definitions = scene(input({ query })).definitions;
  const hits = definitions.filter((definition) =>
    matches(review.definitions.get(definition.id)!, query),
  );
  assert.ok(hits.length > 0 && hits.length < definitions.length);
  for (const definition of definitions) {
    const hit = matches(review.definitions.get(definition.id)!, query);
    assert.equal(definition.classes.includes("dim"), !hit, definition.id);
  }
});

test("an empty query changes nothing: neighbours still dim, and the current never does", () => {
  const definitions = scene(input({ query: "" })).definitions;
  const dim = (id: string) =>
    definitions
      .find((definition) => definition.id === id)!
      .classes.includes("dim");
  assert.ok(!dim(here));
  assert.ok(definitions.some((definition) => dim(definition.id)));
  assert.equal(
    definitions.filter((definition) => dim(definition.id)).length,
    scene(input()).definitions.filter((definition) =>
      definition.classes.includes("dim"),
    ).length,
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

test("hovering a group lights exactly that group", () => {
  const key = every(laid.groups).at(-1)!.key;
  const lit = scene(input({ over: key })).groups.filter((group) => group.lit);
  assert.deepEqual(
    lit.map((group) => group.key),
    [key],
  );
  assert.ok(scene(input()).groups.every((group) => !group.lit));
});
