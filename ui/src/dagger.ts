/* The shapes dagger emits, as far as the page cares about them.
 *
 * Only what's read here is described. A protocol is dagger's to define and this is one
 * reader of it, so claiming more than is used would be inventing a second, quietly
 * disagreeing copy of the model.
 */

export type Identity = number;

export interface Span {
  start: number;
  end: number;
}

export interface Piece {
  text: string;
  span: Span;
  file?: string;
}

export interface Locator {
  scope: string[];
  name: string;
}

/** Everything true of a definition in one snapshot. */
export interface Occurrence {
  locator: Locator;
  file: string;
  kind: string;
  contract?: string;
  parts: Record<string, Piece[]>;
}

/** Added, removed, or kept with a before and an after. */
export interface Sides {
  added?: Occurrence;
  removed?: Occurrence;
  kept?: { before: Occurrence; after: Occurrence };
}

export interface RawDefinition {
  identity: Identity;
  sides: Sides;
}

export interface Edits {
  contract: boolean;
  moved: boolean;
  parts: string[];
}

export type Change = "added" | "removed" | { kept: Edits };

export interface Edge {
  from: Identity;
  to: Identity;
  via?: Identity[];
  part?: string | null;
}

export interface Cost {
  peak_open: number;
  total_open: number;
  taken_on_faith: number;
  jumps: number;
}

export interface Step {
  definition: Identity;
  on_faith: Identity[];
}

/** What dagger says it had to work around, one key naming the kind. */
export type Diagnostic = Record<string, any>;

export interface Note {
  message: string;
  file?: string | null;
}

export interface Raw {
  definitions: RawDefinition[];
  review: {
    edges: Edge[];
    changes: Record<Identity, Change>;
    affected: Identity[];
    diagnostics: Diagnostic[];
  };
  ordering: { steps: Step[]; cost: Cost };
  grouping: { name?: string; of?: Record<Identity, string[]> };
  notes: Note[];
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

export interface Review {
  definitions: Map<Identity, Definition>;
  steps: Step[];
  edges: Edge[];
  changes: Record<Identity, Change>;
  cost: Cost;
  grouping?: string;
  worries: string[];
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
