/* Arrow keys move by this much; a drag moves by however far the pointer went. */
const NUDGE = 24;

/** The drag handle on the sheet's inner edge. Reports the width the sheet should be. */
export function Resizer(props: { onResize: (width: number) => void }) {
  /* Pointer capture so releasing outside the handle still ends the drag. */
  const widen = (event: PointerEvent) => {
    const edge = event.currentTarget as HTMLElement;
    edge.setPointerCapture(event.pointerId);

    const move = (moved: PointerEvent) =>
      props.onResize(window.innerWidth - moved.clientX);
    const done = () => {
      edge.removeEventListener("pointermove", move);
      edge.removeEventListener("pointerup", done);
      edge.removeEventListener("pointercancel", done);
    };

    edge.addEventListener("pointermove", move);
    edge.addEventListener("pointerup", done);
    edge.addEventListener("pointercancel", done);
  };

  /* The sheet sits on the right, so left is wider. */
  const nudge = (event: KeyboardEvent) => {
    const by =
      event.key === "ArrowLeft"
        ? NUDGE
        : event.key === "ArrowRight"
          ? -NUDGE
          : 0;
    if (!by) return;
    const sheet = (event.currentTarget as HTMLElement).parentElement!;
    props.onResize(sheet.offsetWidth + by);
    event.preventDefault();
  };

  return (
    <div
      class="wider"
      role="separator"
      aria-orientation="vertical"
      aria-label="Resize"
      tabindex="0"
      onPointerDown={widen}
      onKeyDown={nudge}
    />
  );
}
