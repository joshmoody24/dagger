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

export const MARK = {
  added: "+",
  removed: "-",
  type: "!",
  body: "~",
  docs: '"',
  reached: "=",
  still: ".",
};
export const TINT = {
  added: "add",
  removed: "del",
  type: "chg",
  body: "chg",
  docs: "chg",
  reached: "reached",
  still: "reached",
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
    mark: marking(one),
    reached: one.reached ?? 0,
    parent: one.parent,
  };
}

/* One word for what happened, which is all a node has room for. */
function marking(one: RawDefinition): Mark {
  const change = one.change;
  if (change === "added") return "added";
  if (change === "removed") return "removed";

  const edits = change.kept;
  if (edits.type_changed) return "type";
  if (edits.parts.includes("body") || edits.parts.includes("type"))
    return "body";
  if (edits.parts.includes("docs")) return "docs";
  return one.reached !== null ? "reached" : "still";
}

/** Whether a change to this one means its callers have to change too. */
export function broke(review: Review, id: Identity) {
  const mark = review.definitions.get(id)?.mark;
  return mark === "removed" || mark === "type";
}
