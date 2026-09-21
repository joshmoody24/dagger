/* The shapes dagger emits, as far as the page cares about them.
 *
 * Only what's read here is described. A protocol is dagger's to define and this is one
 * reader of it, so claiming more than is used would be inventing a second, quietly
 * disagreeing copy of the model.
 */

import type * as said from "./types.generated.ts";

/* What dagger says is written from the types that say it, in types.generated.ts, and named here so
 * the page has one place to look. What the page makes of it is further down and written by
 * hand, because nothing in Rust knows about a box on a screen. */
export type Identity = said.Identity;
export type Span = said.Span;
export type Piece = said.Piece;
export type Locator = said.Locator;
export type Occurrence = said.Occurrence;
export type Sides = said.Sides;
export type Change = said.Change;
export type Edits = said.Edits;
export type Edge = said.Edge;
export type Cost = said.Cost;
export type Step = said.Step;
export type Diagnostic = said.Diagnostic;
export type Note = said.Note;

/** One definition as it arrives: an identity, and what it was on each side. */
export type RawDefinition = said.Definition;

/** Everything one reading of a repository comes to. */
export interface Raw {
  definitions: said.Definition[];
  review: said.Review;
  ordering: said.Ordering;
  grouping: said.Grouping;
  notes: said.Note[];
}

/* ---------------- what the page makes of it ---------------- */

export type Mark = "added" | "removed" | "contract" | "body" | "docs" | "affected" | "still";

/** One definition, in the shape a page wants rather than the shape it arrived in. */
export interface Definition {
  id: Identity;
  name: string;
  scope: string[];
  path: string;
  file: string;
  kind: string;
  before: Occurrence | null;
  after: Occurrence | null;
  mark: Mark;
  group: string[];
}

/* Something dagger had to work around, and whether it might have cost the reader a change.
 * Both go on the page; only one of them is a warning. */
export interface Worry {
  said: string;
  hides: boolean;
}

export interface Review {
  definitions: Map<Identity, Definition>;
  steps: Step[];
  edges: Edge[];
  changes: Record<Identity, Change>;
  affected: Set<Identity>;
  cost: Cost;
  grouping?: string;
  worries: Worry[];
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

/** A place holding definitions, and other places. */
export interface Box {
  key: string;
  label: string;
  module: Definition | null;
  rows: Definition[][];
  lanes: Box[][];
  boxes: Box[];
  w: number;
  h: number;
  x?: number;
  y?: number;
}

export interface Laid {
  at: Map<Identity, Spot>;
  boxes: Box[];
  modules: Map<string, Definition>;
  w: number;
  h: number;
}
