import { onCleanup, onMount } from "solid-js";

/* A key typed into a field is for the field, not the page. */
export const editing = (target: EventTarget | null) =>
  target instanceof HTMLElement &&
  (target.isContentEditable ||
    ["INPUT", "TEXTAREA", "SELECT"].includes(target.tagName));

export const modified = (event: KeyboardEvent) =>
  event.metaKey || event.ctrlKey || event.altKey;

/* Listeners live from mount to cleanup, so a component that unmounts stops hearing. */
export function listen<K extends keyof DocumentEventMap>(
  target: Document,
  type: K,
  heard: (event: DocumentEventMap[K]) => void,
): void;
export function listen<K extends keyof WindowEventMap>(
  target: Window,
  type: K,
  heard: (event: WindowEventMap[K]) => void,
): void;
export function listen(
  target: EventTarget,
  type: string,
  heard: (event: any) => void,
) {
  onMount(() => {
    target.addEventListener(type, heard);
    onCleanup(() => target.removeEventListener(type, heard));
  });
}

export interface ReviewActions {
  readNext: () => void;
  readBack: () => void;
  next: () => void;
  back: () => void;
  toggleRead: () => void;
  toggleSheet: () => void;
  ripples: () => void;
  theme: () => void;
  first: () => void;
  last: () => void;
  search: () => void;
  /** Returns false when something else (an open dialog) owns Escape. */
  escape: () => boolean | void;
}

/* Same keys as the CLI. Global, so nothing needs focus. */
const KEYS: Record<string, keyof ReviewActions> = {
  /* h/l both mark as read: going back means you're done with the current one too. */
  l: "readNext",
  h: "readBack",
  /* n/p step without marking as read. */
  n: "next",
  p: "back",
  ArrowDown: "next",
  ArrowUp: "back",
  " ": "next",
  m: "toggleRead",
  "[": "toggleSheet",
  r: "ripples",
  t: "theme",
  g: "first",
  G: "last",
  "/": "search",
  Escape: "escape",
};

export function useReviewKeys(act: ReviewActions) {
  listen(document, "keydown", (event) => {
    if (modified(event) || editing(event.target)) return;
    const pressed = KEYS[event.key];
    if (!pressed) return;
    if (act[pressed]() === false) return;
    event.preventDefault();
  });
}

/* Held from keydown to keyup rather than acting per key repeat, because repeat timing
 * (delay, then bursts) makes a scroll stutter. Window blur releases too, since the keyup
 * will never arrive. */
export function useHeldKeys<T>(
  keys: Record<string, T>,
  hold: (which: T) => void,
  release: (which: T) => void,
) {
  listen(document, "keydown", (event) => {
    if (modified(event) || editing(event.target)) return;
    if (!(event.key in keys)) return;
    if (!event.repeat) hold(keys[event.key]);
    event.preventDefault();
  });
  listen(document, "keyup", (event) => {
    if (event.key in keys) release(keys[event.key]);
  });
  listen(window, "blur", () => Object.values(keys).forEach(release));
}
