/* The shapes dagger emits, only as far as the page reads them. The protocol is dagger's;
 * describing more than is used would be a second, disagreeing copy of it.
 */

import type * as wire from "./types.generated.ts";

/* Wire types come from types.generated.ts and are re-exported here so the page has one
 * place to look. The page's own shapes are below. */
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

/* Aliased to the generated type so a renamed field fails to compile instead of arriving
 * undefined. */
export type Raw = wire.Review;

/* ---------------- what the page makes of it ---------------- */

export type Mark =
  "added" | "removed" | "contract" | "body" | "docs" | "affected" | "still";

/* One definition in the shape the page wants: name flattened, a one-word mark, and both
 * sides pulled out of the wire shape. */
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

/* A gap between two pieces of a definition is a line on the page but not in the file, so it
 * has no number. */
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
  h: number;
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
