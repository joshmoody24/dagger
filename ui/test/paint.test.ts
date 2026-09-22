/* The canvas painter, run against a recording context so it can be tested without a
 * browser. What it draws for the saved review is kept in paint.golden.json; a change to
 * the drawing shows up as a diff there. Regenerate with UPDATE_GOLDEN=1.
 */

import assert from "node:assert/strict";
import fs from "node:fs";
import test from "node:test";
import type { Raw } from "../src/dagger.ts";
import { digest } from "../src/digest.ts";
import { paint, type Palette, type Scene } from "../src/graph/paint.ts";
import { NODE_H, layout } from "../src/layout.ts";

const raw: Raw = JSON.parse(
  fs.readFileSync(new URL("./review.json", import.meta.url), "utf8"),
);
const review = digest(raw);
const laid = layout(review);

type Call = [string, ...unknown[]];

/* Every method call and property write, in order. Text is measured at 0.6em per character,
 * the same estimate the layout uses. */
function recording(): { ink: CanvasRenderingContext2D; calls: Call[] } {
  const calls: Call[] = [];
  const ink = new Proxy({} as CanvasRenderingContext2D, {
    get: (_, name: string) =>
      name === "measureText"
        ? (text: string) => ({ width: text.length * 8.4 })
        : (...args: unknown[]) => {
            calls.push([name, ...args]);
          },
    set: (_, name: string, value: unknown) => {
      calls.push([`${name}=`, value]);
      return true;
    },
  });
  return { ink, calls };
}

const tint: Palette = Object.fromEntries(
  [
    "paper",
    "ink",
    "muted",
    "faint",
    "rule",
    "lean",
    "path",
    "add",
    "del",
    "chg",
    "aff",
  ].map((name) => [name, `--${name}`]),
);

const scene = (over: Partial<Scene> = {}): Scene => ({
  review,
  laid,
  here: review.steps[0]?.definition ?? null,
  next: review.steps[1]?.definition ?? null,
  read: new Set(),
  over: null,
  touching: null,
  view: { k: 1, x: 0, y: 0 },
  room: { width: 1200, height: 800 },
  dense: 1,
  ...over,
});

test("every node and box is drawn exactly once, and the current node is lit", () => {
  const { ink, calls } = recording();
  paint(ink, scene(), tint);

  const count = (of: string) => calls.filter(([name]) => name === of).length;
  const boxes = (() => {
    let seen = 0;
    const walk = (box: (typeof laid.boxes)[number]) => {
      seen += 1;
      box.boxes.forEach(walk);
    };
    laid.boxes.forEach(walk);
    return seen;
  })();

  /* A box writes its label; a node writes its mark and its name over a filled rect. */
  assert.equal(count("fillText"), boxes + 2 * laid.at.size);
  // One fill per node, plus the arrowhead pointing at the next step.
  assert.equal(count("fill"), laid.at.size + 1);

  const lean = calls.findIndex(
    ([name, value]) => name === "strokeStyle=" && value === "--lean",
  );
  assert.ok(lean >= 0, "the current node is outlined in the lean colour");
});

test("nothing is drawn outside the laid-out page", () => {
  const { ink, calls } = recording();
  paint(ink, scene(), tint);
  for (const [name, , x, y] of calls) {
    if (name !== "fillText") continue;
    assert.ok((x as number) >= 0 && (x as number) <= laid.w, `x ${x}`);
    assert.ok((y as number) >= 0 && (y as number) <= laid.h + NODE_H, `y ${y}`);
  }
});

/* Locks the drawing as a whole. Rounded to whole pixels so a platform's float noise
 * doesn't count as a change. */
test("the saved review paints the same as before", () => {
  const { ink, calls } = recording();
  paint(ink, scene(), tint);
  const rounded = calls.map((call) =>
    call.map((value) =>
      typeof value === "number" ? Math.round(value) : value,
    ),
  );
  const golden = new URL("./paint.golden.json", import.meta.url);
  if (process.env.UPDATE_GOLDEN || !fs.existsSync(golden)) {
    fs.writeFileSync(golden, JSON.stringify(rounded));
  }
  assert.deepEqual(rounded, JSON.parse(fs.readFileSync(golden, "utf8")));
});
