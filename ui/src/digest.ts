import type { Definition, Mark, RawDefinition, Identity, Raw, Review } from "./dagger.ts";

/* Turns what dagger sent into the shapes a page wants: one word for what happened to each
 * definition, and the two sides pulled out of a shape built so "in neither" can't be
 * written. Nothing here decides anything about the code — that arrives already settled.
 */

export const MARK = {
  added: "+", removed: "−", contract: "!", body: "~", docs: '"', affected: "≈", still: "·",
};
export const TINT = {
  added: "add", removed: "del", contract: "chg", body: "chg", docs: "chg", affected: "aff", still: "aff",
};

/* Nearly nothing, now.
 *
 * This used to gather one definition's facts from six places — a map for what changed,
 * another for how far a change reached it, another for its group — and word every warning
 * itself. All of that arrives already worked out and already said, so what's left is the
 * page's own business: flattening a name for display, choosing the one word a node has room
 * for, and pulling the two sides out of a shape built so "in neither" can't be written.
 */
export function digest(raw: Raw): Review {
  const definitions = new Map<Identity, Definition>();
  for (const [id, one] of Object.entries(raw.definitions)) {
    /* Which sides there are is the one thing the model won't let you get wrong: a
     * definition is added, removed, or kept with both — never neither. Asked this way
     * rather than by reaching for a field, the compiler holds that to it. */
    const sides = one.sides;
    const before = "kept" in sides ? sides.kept.before : "removed" in sides ? sides.removed : null;
    const after = "kept" in sides ? sides.kept.after : "added" in sides ? sides.added : null;
    const shown = (after || before)!;

    definitions.set(id, {
      id,
      name: shown.locator.name,
      scope: shown.locator.scope,
      path: [...shown.locator.scope, shown.locator.name].join("::"),
      file: shown.file,
      kind: shown.kind,
      role: one.role,
      change: one.change,
      before,
      after,
      mark: marking(one),
      away: one.reached ?? 0,
      parent: one.parent,
      group: one.group ?? [],
    });
  }

  return {
    definitions,
    steps: raw.reading,
    edges: raw.edges,
    ripples: raw.ripples,
    cost: raw.cost,
    grouping: raw.grouping ?? undefined,
    bands: new Map(raw.groups.map((group) => [group.path.join("/"), group.band])),
    warnings: raw.warnings,
  };
}

/* One word for what happened, which is all a node has room for. */
function marking(one: RawDefinition): Mark {
  const change = one.change;
  if (change === "added") return "added";
  if (change === "removed") return "removed";

  const edits = change.kept;
  if (edits.contract) return "contract";
  if (edits.parts.includes("body") || edits.parts.includes("type")) return "body";
  if (edits.parts.includes("docs")) return "docs";
  return one.reached !== null ? "affected" : "still";
}

/** Whether a change to this one means its callers have to change too. */
export function broke(review: Review, id: Identity) {
  const change = review.definitions.get(id)?.change;
  if (change === "removed") return true;
  if (change === undefined || change === "added") return false;
  return Boolean(change.kept && change.kept.contract);
}
