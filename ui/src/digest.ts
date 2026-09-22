import { LEGEND } from "./types.generated.ts";
import type {
  Definition,
  Mark,
  RawDefinition,
  Identity,
  Raw,
  Review,
} from "./dagger.ts";

/* Turns what dagger sent into the shapes the page wants. Nothing here decides anything
 * about the code; that arrives already settled.
 */

export const MARK = Object.fromEntries(
  LEGEND.map(([mark, glyph]) => [mark, glyph]),
) as Record<Mark, string>;
export const TINT = {
  added: "add",
  removed: "del",
  type: "chg",
  body: "chg",
  docs: "chg",
  reached: "reached",
  untouched: "reached",
};

export function digest(raw: Raw): Review {
  return {
    title: raw.title,
    definitions: new Map(
      Object.entries(raw.definitions).map(([id, one]) => [
        id,
        digested(id, one),
      ]),
    ),
    steps: raw.reading,
    edges: raw.edges,
    ripples: raw.ripples,
    cost: raw.cost,
    grouping: raw.grouping ?? undefined,
    groups: raw.groups,
    warnings: raw.warnings,
  };
}

function digested(id: Identity, one: RawDefinition): Definition {
  /* Checked by discriminant so the compiler enforces added, removed or kept, never
   * neither. */
  const sides = one.sides;
  const before =
    "kept" in sides
      ? sides.kept.before
      : "removed" in sides
        ? sides.removed
        : null;
  const after =
    "kept" in sides ? sides.kept.after : "added" in sides ? sides.added : null;
  const shown = (after || before)!;

  return {
    id,
    name: shown.locator.name,
    scope: shown.locator.scope,
    locator: [...shown.locator.scope, shown.locator.name].join("::"),
    file: shown.file,
    kind: shown.kind,
    change: one.change,
    before,
    after,
    mark: one.mark,
    reached: one.reached ?? 0,
    parent: one.parent,
  };
}

/** Whether a change to this one means its callers have to change too. */
export function broke(review: Review, id: Identity) {
  const mark = review.definitions.get(id)?.mark;
  return mark === "removed" || mark === "type";
}
