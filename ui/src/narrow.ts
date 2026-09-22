import type { Review } from "./dagger.ts";

/** The review with definitions reached further than `ripples` hops out left off. */
export function narrowed(whole: Review, ripples: number): Review {
  if (ripples >= whole.ripples) return whole;

  const gone = new Set(
    [...whole.definitions.values()]
      .filter((one) => one.mark === "reached" && one.reached > ripples)
      .map((one) => one.id),
  );
  if (!gone.size) return whole;

  return {
    ...whole,
    definitions: new Map(
      [...whole.definitions].filter(([id]) => !gone.has(id)),
    ),
    steps: whole.steps.filter((step) => !gone.has(step.definition)),
    edges: whole.edges.filter(
      (edge) => !gone.has(edge.from) && !gone.has(edge.to),
    ),
  };
}
