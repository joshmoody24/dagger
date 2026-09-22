import type { Review } from "./dagger.ts";

/** The review with affected definitions further than `far` ripples out left off. */
export function narrowed(whole: Review, far: number): Review {
  if (far >= whole.ripples) return whole;

  const gone = new Set(
    [...whole.definitions.values()]
      .filter((one) => one.mark === "affected" && one.away > far)
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
