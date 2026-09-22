import type { Box, Identity, Laid, Spot } from "../dagger.ts";

export interface Point {
  x: number;
  y: number;
}

export type Touched = { node: Identity } | { box: Box };

/** Every box in the layout, however deeply nested. */
export const every = (boxes: Box[]): Box[] =>
  boxes.flatMap((box) => [box, ...every(box.boxes)]);

/* Nodes win over boxes; the innermost box wins over its parents. `boxes` is `every(laid.boxes)`,
 * flattened once by the caller rather than per pointer move. */
export function hit(laid: Laid, boxes: Box[], { x, y }: Point): Touched | null {
  const covers = (spot: Spot) =>
    x >= spot.x && x <= spot.x + spot.w && y >= spot.y && y <= spot.y + spot.h;

  const node = [...laid.at].find(([, spot]) => covers(spot));
  if (node) return { node: node[0] };

  const innermost = boxes
    .filter(covers)
    .reduce<Box | null>(
      (best, box) => (!best || box.w * box.h < best.w * best.h ? box : best),
      null,
    );
  return innermost ? { box: innermost } : null;
}
