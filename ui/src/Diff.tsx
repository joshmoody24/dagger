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
import { compare, focused, paired, type Range } from "./diff.ts";
import { broke, TINT } from "./digest.ts";
import { stitch } from "./text.ts";
import { colouring, painted, readied, speaks } from "./colouring.ts";
import { dressing, wearing } from "./theme.ts";
import "./Diff.css";

export type Names = Map<string, Identity>;

export function Diff(props: {
  definition: Def;
  names: Names;
  review: Review;
  onOpen: (id: Identity) => void;
}) {
  createEffect(() =>
    readied(speaks(props.definition.file), dressing(), wearing()),
  );

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
    gaps: Set<number>;
  }>({ of: null, gaps: new Set() });
  const opened = () =>
    unfolded().of === props.definition.id ? unfolded().gaps : new Set<number>();
  const unfold = (from: number) =>
    setUnfolded({
      of: props.definition.id,
      gaps: new Set([...opened(), from]),
    });

  const lines = createMemo(() =>
    cut().flatMap((one) =>
      one.gap && opened().has(one.gap.from)
        ? all().slice(one.gap.from, one.gap.to)
        : [one],
    ),
  );

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
                onClick={() => unfold(gap().from)}
              >
                {gap().to - gap().from} unchanged lines
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
  const parts = createMemo((): Led[] =>
    stressed(props.pieces, props.emphasis).flatMap((piece) =>
      piece.colour ? split(piece, props.names, props.here) : [piece],
    ),
  );

  const marked = (part: Led) =>
    part.goes && props.review.definitions.get(part.goes)?.mark;
  const classes = (part: Led) => {
    const mark = marked(part);
    return [
      part.goes && "link",
      mark && TINT[mark],
      part.goes && broke(props.review, part.goes) && "broke",
      part.changed && "changed",
    ]
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
