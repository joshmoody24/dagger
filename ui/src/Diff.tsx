import {
  createEffect,
  createMemo,
  createSignal,
  For,
  on,
  Show,
} from "solid-js";
import type { Definition as Def, Identity, Shown } from "./dagger.ts";
import type { Painted } from "./colouring.ts";
import { compare, focused } from "./diff.ts";
import { stitch } from "./text.ts";
import { colouring, painted, readied, speaks } from "./colouring.ts";
import { dressing, wearing } from "./theme.ts";
import "./Diff.css";

export type Names = Map<string, Identity>;

export function Diff(props: {
  definition: Def;
  names: Names;
  onOpen: (id: Identity) => void;
}) {
  createEffect(() =>
    readied(speaks(props.definition.file), dressing(), wearing()),
  );

  /* Memoised: read once per rendered line, and diffing/colouring is expensive. */
  const lines = createMemo(() => {
    const [was, is] = [
      stitch(props.definition.before),
      stitch(props.definition.after),
    ];
    const all: Shown[] =
      was && is
        ? compare(was, is)
        : [
            ...(was ?? []).map((line) => ({ mark: "−" as const, line })),
            ...(is ?? []).map((line) => ({ mark: "+" as const, line })),
          ];
    return focused(all);
  });

  const unchanged = createMemo(
    () => lines().length > 0 && lines().every((one) => one.mark === " "),
  );

  /* Highlighted in one pass so multi-line tokens (strings, comments) are handled. */
  const tinted = createMemo(() => {
    void colouring();
    return painted(
      lines().map((one) => one.line.text),
      speaks(props.definition.file),
      wearing(),
    );
  });

  const gutter = () => {
    const most = Math.max(0, ...lines().map((one) => one.line.at ?? 0));
    return `${Math.max(3, String(most).length)}ch`;
  };

  /* Scroll to the first changed line, or a long definition opens showing no change. The
   * lines are re-rendered whenever they change, so the ref is fresh when the effect runs. */
  const firstChanged = createMemo(() =>
    lines().findIndex((one) => one.mark !== " "),
  );
  const [first, setFirst] = createSignal<HTMLElement | null>(null);
  let code!: HTMLPreElement;

  createEffect(
    on([lines, first], ([shown, changed]) => {
      const pane = code.parentElement;
      if (!pane) return;
      if (!changed || !shown.some((one) => one.mark !== " ")) {
        pane.scrollTop = 0;
        return;
      }
      const above = pane.getBoundingClientRect().top;
      const onto = changed.getBoundingClientRect().top;
      pane.scrollTop += onto - above - 12;
    }),
  );

  return (
    <pre
      class={`code${unchanged() ? " same" : ""}`}
      style={{ "--gutter": gutter() }}
      ref={code}
    >
      <For each={lines()}>
        {(one, at) => (
          <span
            class={`line ${one.mark === "+" ? "added" : one.mark === "−" ? "removed" : ""}`}
            ref={(line) => at() === firstChanged() && setFirst(line)}
          >
            <span class="marker" aria-hidden="true">
              {one.mark === " " ? "" : one.mark}
            </span>
            <span class="gutter" aria-hidden="true">
              {one.line.at ?? ""}
            </span>
            <Show
              when={one.line.at !== null}
              fallback={<span class="gap">…</span>}
            >
              <Code
                pieces={tinted()[at()] ?? [{ text: one.line.text }]}
                names={props.names}
                here={props.definition.id}
                onOpen={props.onOpen}
              />
            </Show>
          </span>
        )}
      </For>
    </pre>
  );
}

/* Post-processes highlighter output to link names that are definitions in this review,
 * which no highlighting library can do on its own. */
function Code(props: {
  pieces: Painted[];
  names: Names;
  here: Identity;
  onOpen: (id: Identity) => void;
}) {
  const parts = createMemo((): Led[] =>
    props.pieces.flatMap((piece) =>
      piece.colour ? split(piece, props.names, props.here) : [piece],
    ),
  );

  return (
    <For each={parts()}>
      {(part) => (
        <Show
          when={part.goes}
          fallback={
            <span style={part.colour ? { color: part.colour } : undefined}>
              {part.text}
            </span>
          }
        >
          {(goes) => (
            // A span, not a button, so the code stays selectable as text.
            <span
              role="link"
              tabindex="0"
              class="link"
              style={part.colour ? { color: part.colour } : undefined}
              onClick={() => props.onOpen(goes())}
              onKeyDown={(event) => {
                if (event.key === "Enter") props.onOpen(goes());
              }}
            >
              {part.text}
            </span>
          )}
        </Show>
      )}
    </For>
  );
}

type Led = Painted & { goes?: Identity };

function split(piece: Painted, names: Names, here: Identity): Led[] {
  const goes = names.get(piece.text.trim());
  if (goes !== undefined && goes !== here && piece.text.trim() === piece.text) {
    return [{ ...piece, goes }];
  }
  return [piece];
}
