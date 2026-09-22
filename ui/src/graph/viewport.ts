import { select } from "d3-selection";
import { zoom as zooming, zoomIdentity, zoomTransform } from "d3-zoom";
import type { Laid, Spot } from "../dagger.ts";

const EDGE = 24;
/* Max zoom. Past this the text is too big to be useful. */
export const CLOSEST = 4;

interface Viewing {
  paper: () => HTMLCanvasElement;
  room: () => DOMRect;
  laid: () => Laid;
  onZoom: () => void;
}

/* The d3 zoom behaviour is the single owner of the view transform; nothing sets it
 * directly. */
export function viewport(of: Viewing) {
  const behaviour = zooming<HTMLCanvasElement, unknown>()
    /* Wheel is handled by onWheel instead. */
    .filter(
      (event: Event & { ctrlKey?: boolean; button?: number }) =>
        event.type !== "wheel" && !event.ctrlKey && !event.button,
    )
    .on("zoom", () => of.onZoom());

  const seen = () => zoomTransform(of.paper());
  const canvas = () => select(of.paper());

  /* Fit-all transform. Also the minimum zoom. */
  const whole = () => {
    const room = of.room();
    const laid = of.laid();
    const k = Math.min(
      (room.width - 2 * EDGE) / laid.w,
      (room.height - 2 * EDGE) / laid.h,
    );
    return zoomIdentity
      .translate((room.width - laid.w * k) / 2, (room.height - laid.h * k) / 2)
      .scale(k);
  };

  /* Re-run whenever the layout changes size (e.g. ripples toggled), or the old extents
   * keep constraining the view. */
  const bounded = () => {
    const laid = of.laid();
    behaviour.translateExtent([
      [0, 0],
      [laid.w, laid.h],
    ]);
    behaviour.scaleExtent([whole().k, CLOSEST]);
  };

  const fit = () => {
    bounded();
    canvas().call(behaviour.transform, whole());
  };

  /* Centred on a spot at zoom k. */
  const onto = (spot: Spot, k: number) => {
    const room = of.room();
    canvas().call(
      behaviour.transform,
      zoomIdentity
        .translate(room.width / 2, room.height / 2)
        .scale(k)
        .translate(-(spot.x + spot.w / 2), -(spot.y + spot.h / 2)),
    );
  };

  const scaleBy = (by: number, towards?: [number, number]) =>
    behaviour.scaleBy(canvas(), by, towards);

  /* Touchpads fire wheel events far faster than we can draw, and d3's built-in wheel
   * handling is non-passive and recomputes per event, which lags. Batch deltas into one
   * scale change per frame; passive is fine since the page never scrolls. */
  let wheeled = 0;
  let towards: [number, number] = [0, 0];
  let turning = 0;

  const onWheel = (event: WheelEvent) => {
    const room = of.room();
    towards = [event.clientX - room.left, event.clientY - room.top];
    wheeled += event.deltaY;
    if (turning) return;

    turning = requestAnimationFrame(() => {
      turning = 0;
      const by = Math.pow(0.9985, wheeled);
      wheeled = 0;
      scaleBy(by, towards);
    });
  };

  /* Where a pointer event lands in layout coordinates. */
  const pointing = (event: { clientX: number; clientY: number }) => {
    const room = of.room();
    const view = seen();
    return {
      x: (event.clientX - room.left - view.x) / view.k,
      y: (event.clientY - room.top - view.y) / view.k,
    };
  };

  /* Whether a spot is entirely on screen at the current view. */
  const shows = (spot: Spot) => {
    const view = seen();
    const room = of.room();
    const [x, y] = [view.applyX(spot.x), view.applyY(spot.y)];
    return (
      x >= 0 &&
      y >= 0 &&
      x + spot.w * view.k <= room.width &&
      y + spot.h * view.k <= room.height
    );
  };

  const attach = () => canvas().call(behaviour);

  return {
    attach,
    seen,
    whole,
    bounded,
    fit,
    onto,
    scaleBy,
    onWheel,
    pointing,
    shows,
  };
}
