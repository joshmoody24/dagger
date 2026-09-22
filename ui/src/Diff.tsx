import {
  createEffect,
  createMemo,
  createSignal,
  For,
  on,
  Show,
} from "solid-js";
import type { Definition as Def, Identity, Review, Shown } from "./dagger.ts";
import type { Painted } from "./colouring.ts";
import { compare, focused, paired, type Detailed, type Range } from "./diff.ts";
import { TINT } from "./digest.ts";
import { stitch } from "./text.ts";
import { colouring, painted, readied, speaks } from "./colouring.ts";
import { editing, listen, modified } from "./keys.ts";
import { dressing, wearing } from "./theme.ts";
import "./Diff.css";

export type Names = Map<string, Identity>;

/** Lines a click on a collapsed run opens. */
export const STEP = 20;

export function Diff(props: {
  definition: Def;
  names: Names;
  review: Review;
  onOpen: (id: Identity) => void;
}) {
  /* Re-asked after every load as well, so a grammar that failed once is tried again
   * rather than leaving this language plain for the rest of the session. */
  createEffect(() => {
    void colouring();
    void readied(speaks(props.definition.file), dressing(), wearing());
  });

  /* Names link only within one language: `new` in TypeScript is a keyword, not the Rust
   * `fn new` that happens to share its spelling. */
  const linkable = createMemo(() => {
    const language = speaks(props.definition.file);
    return new Map(
      [...props.names].filter(([, id]) => {
        const target = props.review.definitions.get(id);
        return target !== undefined && speaks(target.file) === language;
      }),
    );
  });

  /* Memoised: read once per rendered line, and diffing/colouring is expensive. */
  const all = createMemo(() => {
    const [was, is] = [
      stitch(props.definition.before),
      stitch(props.definition.after),
    ];
    const shown: Shown[] =
      was && is
        ? compare(was, is)
        : [
            ...(was ?? []).map((line) => ({ mark: "−" as const, line })),
            ...(is ?? []).map((line) => ({ mark: "+" as const, line })),
          ];
    return paired(shown);
  });
  const cut = createMemo(() => focused(all()));

  /* Gaps unfolded, by where each starts; remembered with the definition so another one's
   * gaps start folded without an effect to reset them. */
  const [unfolded, setUnfolded] = createSignal<{
    of: Identity | null;
    /* How many lines of each gap have been opened, by where the gap starts. */
    gaps: Map<number, number>;
  }>({ of: null, gaps: new Map() });
  const opened = () =>
    unfolded().of === props.definition.id
      ? unfolded().gaps
      : new Map<number, number>();
  /* A click opens the next stretch from the top of the gap, the way a forge does, so a
   * long gap can be opened a bit at a time. */
  const unfold = (from: number) =>
    setUnfolded({
      of: props.definition.id,
      gaps: new Map([...opened(), [from, (opened().get(from) ?? 0) + STEP]]),
    });

  const lines = createMemo(() =>
    cut().flatMap((one): Detailed[] => {
      if (!one.gap) return [one];
      const upto = Math.min(one.gap.from + shown(one.gap), one.gap.to);
      return [
        ...all().slice(one.gap.from, upto),
        ...(upto < one.gap.to ? [one] : []),
      ];
    }),
  );
  /* How much of a gap has been opened so far. The gap keeps its original key so a second
   * click carries on from where the first left off. */
  const shown = (gap: { from: number }) => opened().get(gap.from) ?? 0;

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

  /* `o` opens the first collapsed run in view, the way you'd meet it scrolling down.
   * Repeats, so holding it unrolls a long one. */
  listen(document, "keydown", (event) => {
    if (event.key !== "o" || modified(event) || editing(event.target)) return;
    const pane = code.parentElement;
    if (!pane) return;
    const top = pane.getBoundingClientRect().top;
    const folds = [...code.querySelectorAll<HTMLButtonElement>("button.fold")];
    const next = folds.find(
      (fold) => fold.getBoundingClientRect().bottom >= top,
    );
    if (next) {
      next.click();
      event.preventDefault();
    }
  });

  /* Unfolding a gap re-renders too, and mustn't scroll away from what was just opened. */
  createEffect(
    on(cut, (shown) => {
      const [pane, changed] = [code.parentElement, first()];
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
          <Show
            when={one.gap}
            fallback={
              <span
                class={`line ${one.mark === "+" ? "added" : one.mark === "−" ? "removed" : ""}`}
                ref={(line) => at() === firstChanged() && setFirst(line)}
              >
                <span class="marker" aria-hidden="true">
                  {one.mark === " " || one.line.at === null ? "" : one.mark}
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
                    emphasis={one.emphasis ?? []}
                    names={linkable()}
                    review={props.review}
                    here={props.definition.id}
                    onOpen={props.onOpen}
                  />
                </Show>
              </span>
            }
          >
            {(gap) => (
              <button
                type="button"
                class="line fold"
                title={`${gap().to - gap().from - shown(gap())} unchanged lines; opens ${STEP}`}
                onClick={() => unfold(gap().from)}
              >
                <span class="gap">…</span>
              </button>
            )}
          </Show>
        )}
      </For>
    </pre>
  );
}

/* Post-processes highlighter output to link names that are definitions in this review,
 * which no highlighting library can do on its own. */
function Code(props: {
  pieces: Painted[];
  emphasis: Range[];
  names: Names;
  review: Review;
  here: Identity;
  onOpen: (id: Identity) => void;
}) {
  /* Linked whether or not the highlighter has coloured the piece yet: a name is a name
   * before the grammar arrives. */
  const parts = createMemo((): Led[] =>
    stressed(props.pieces, props.emphasis).flatMap((piece) =>
      split(piece, props.names, props.here),
    ),
  );

  const marked = (part: Led) =>
    part.goes && props.review.definitions.get(part.goes)?.mark;
  const classes = (part: Led) => {
    const mark = marked(part);
    return [part.goes && "link", mark && TINT[mark], part.changed && "changed"]
      .filter(Boolean)
      .join(" ");
  };
  /* A change's tint says more than the grammar's colour, so it wins where there is one. */
  const colour = (part: Led) => {
    const mark = marked(part);
    const tinted = mark && TINT[mark] !== "aff";
    return part.colour && !tinted ? { color: part.colour } : undefined;
  };

  return (
    <For each={parts()}>
      {(part) => (
        <Show
          when={part.goes}
          fallback={
            <span class={classes(part) || undefined} style={colour(part)}>
              {part.text}
            </span>
          }
        >
          {(goes) => (
            // A span, not a button, so the code stays selectable as text.
            <span
              role="link"
              tabindex="0"
              class={classes(part)}
              style={colour(part)}
              title={`open ${props.review.definitions.get(goes())?.path ?? part.text}`}
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

type Led = Painted & { goes?: Identity; changed?: boolean };

/* Pieces cut at the edges of the stressed ranges, so a colour can span a stressed word and
 * an unstressed one. */
function stressed(pieces: Painted[], ranges: Range[]): Led[] {
  if (!ranges.length) return pieces;

  const starts = pieces.reduce<number[]>(
    (acc, piece) => [...acc, acc[acc.length - 1] + piece.text.length],
    [0],
  );
  return pieces.flatMap((piece, at) => {
    const [from, to] = [starts[at], starts[at + 1]];
    const edges = [
      ...new Set([
        from,
        ...ranges.flat().filter((edge) => edge > from && edge < to),
        to,
      ]),
    ].sort((a, b) => a - b);
    return edges.slice(1).map((end, k) => {
      const start = edges[k];
      return {
        ...piece,
        text: piece.text.slice(start - from, end - from),
        changed: ranges.some(([x, y]) => x <= start && end <= y),
      };
    });
  });
}

function split(piece: Led, names: Names, here: Identity): Led[] {
  const goes = names.get(piece.text.trim());
  if (goes !== undefined && goes !== here && piece.text.trim() === piece.text) {
    return [{ ...piece, goes }];
  }
  return [piece];
}
