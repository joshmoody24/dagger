/* The shapes dagger emits, as far as the page cares about them.
 *
 * Only what's read here is described. A protocol is dagger's to define and this is one
 * reader of it, so claiming more than is used would be inventing a second, quietly
 * disagreeing copy of the model.
 */

import type * as wire from "./types.generated.ts";

/* What dagger says is written from the types that say it, in types.generated.ts, and named
 * here so the page has one place to look. What the page makes of it is further down and
 * written by hand, because nothing in Rust knows about a box on a screen. */
export type Identity = wire.Identity;
export type Span = wire.Span;
export type Piece = wire.Piece;
export type Locator = wire.Locator;
export type Occurrence = wire.Occurrence;
export type Sides = wire.Sides;
export type Change = wire.Change;
export type Edits = wire.Edits;
export type Edge = wire.Edge;
export type Cost = wire.Cost;
export type Step = wire.Step;
export type Role = wire.Role;
export type Group = wire.Group;
export type Warning = wire.Warning;

/** One definition as it arrives, with everything dagger knows about it. */
export type RawDefinition = wire.Definition;

/* Everything one reading of a repository comes to.
 *
 * Named here rather than described here. This used to be written out by hand, which left
 * the outermost shape — the one every other shape arrives inside — as the one thing nothing
 * checked. A renamed field doesn't fail in TypeScript: the declaration is satisfied and the
 * value turns up undefined. */
export type Raw = wire.Review;

/* ---------------- what the page makes of it ---------------- */

export type Mark =
  "added" | "removed" | "contract" | "body" | "docs" | "affected" | "still";

/* One definition, in the shape a page wants rather than the shape it arrived in.
 *
 * Nearly all of this now arrives already worked out, and what's left is the page's own
 * business: a name flattened for display, a one-word mark to draw, the two sides pulled out
 * of the shape that makes "in neither" unrepresentable. */
export interface Definition {
  id: Identity;
  name: string;
  scope: string[];
  path: string;
  file: string;
  kind: string;
  /** What happened to it. */
  change: Change;
  before: Occurrence | null;
  after: Occurrence | null;
  mark: Mark;
  /** How far out a change reached it, or nought for something no change reached. */
  away: number;
  /** What it's written inside, when that's on the page too. */
  parent: Identity | null;
}

export interface Review {
  /** What the commit under review is called, when the adapter that read it could say. */
  title: string | null;
  definitions: Map<Identity, Definition>;
  steps: Step[];
  edges: Edge[];
  /** How far this reading followed a change, which is as far as the page can offer. */
  ripples: number;
  cost: Cost;
  grouping?: string | undefined;
  /** What nests in what and how it stacks, as dagger arranged it. */
  groups: Group[];
  warnings: Warning[];
}

/* One line as the page shows it: what it says, and where it is in the file. A gap between
 * two pieces of a definition is a line on the page and nowhere in the file, so it has no
 * number. */
export interface Line {
  at: number | null;
  text: string;
}

/** A line in a diff, and what became of it. */
export interface Shown {
  mark: " " | "+" | "−";
  line: Line;
}

export interface Spot {
  x: number;
  y: number;
  w: number;
  h?: number;
}

/** A box on the page: where it is, and what's directly inside it. */
export interface Box {
  key: string;
  label: string;
  x: number;
  y: number;
  w: number;
  h: number;
  boxes: Box[];
  nodes: Identity[];
}

export interface Laid {
  at: Map<Identity, Spot>;
  boxes: Box[];
  w: number;
  h: number;
}
