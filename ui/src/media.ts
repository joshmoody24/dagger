import { createSignal, onCleanup, onMount } from "solid-js";

/* Wide enough for the sheet to sit beside the graph instead of below it. CSS media
 * queries can't read a custom property, so Reading.css repeats this number by hand. */
export const WIDE = 900;

export function createMediaQuery(query: string, initially = true) {
  const [matches, setMatches] = createSignal(initially);

  onMount(() => {
    const room = window.matchMedia(query);
    const settle = () => setMatches(room.matches);
    settle();
    room.addEventListener("change", settle);
    onCleanup(() => room.removeEventListener("change", settle));
  });

  return matches;
}
