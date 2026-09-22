import { createEffect, createMemo, For, Show } from "solid-js";
import type {
  Definition as Def,
  Identity,
  Review,
  Shown,
  Step,
} from "./dagger.ts";
import type { Painted } from "./colouring.ts";
import { MARK, TINT, broke } from "./digest.ts";
import { compare, focused } from "./diff.ts";
import { stitch } from "./text.ts";
import { colouring, painted, readied, speaks } from "./colouring.ts";
import { dressing, wearing } from "./theme.ts";

type Names = Map<string, Identity>;

/** How far the drawer is open on a narrow screen. */
export type Sheet = "closed" | "half" | "full";
/** Where the sheet sits: a column on a wide screen, a drawer on a narrow one. */
export type Facing = "beside" | "away" | Sheet;

interface ReadingProps {
  review: Review;
  here: Identity | null;
  step: Step | undefined;
  at: number;
  names: Names;
  read: boolean;
  sheet: Facing;
  width: number;
  onStep: (by: number) => void;
  onRead: (by: number) => void;
  onOpen: (id: Identity) => void;
  onToggle: () => void;
  onWiden: (width: number) => void;
  /** Exposes the diff pane so the parent's j/k keys can scroll it. */
  onPane: (pane: HTMLDivElement) => void;
  onExpand: () => void;
  onClose: () => void;
}

export function Reading(props: ReadingProps) {
  const definition = () =>
    props.here === null ? undefined : props.review.definitions.get(props.here);
  const leans = () =>
    props.review.edges
      .filter((edge) => edge.from === definition()?.id)
      .map((edge) => edge.to);

  const because = () => leans().filter((id) => broke(props.review, id));
  const uses = () => leans().filter((id) => !broke(props.review, id));

  let sheet: HTMLDivElement | undefined;

  /* Pointer capture so releasing outside the handle still ends the drag. */
  const widen = (event: PointerEvent) => {
    const edge = event.currentTarget as HTMLElement;
    edge.setPointerCapture(event.pointerId);

    const move = (moved: PointerEvent) =>
      props.onWiden(window.innerWidth - moved.clientX);
    const done = () => {
      edge.removeEventListener("pointermove", move);
      edge.removeEventListener("pointerup", done);
      edge.removeEventListener("pointercancel", done);
    };

    edge.addEventListener("pointermove", move);
    edge.addEventListener("pointerup", done);
    edge.addEventListener("pointercancel", done);
  };

  /* Scroll to the first changed line, or a long definition opens showing no change. */
  createEffect(() => {
    const here = definition();
    const pane = sheet;
    if (!here || !pane) return;

    queueMicrotask(() => {
      const changed = pane.querySelector(".ln.a, .ln.r");
      if (!changed) {
        pane.scrollTop = 0;
        return;
      }
      const above = pane.getBoundingClientRect().top;
      const onto = changed.getBoundingClientRect().top;
      pane.scrollTop += onto - above - 12;
    });
  });

  return (
    <Show when={definition()}>
      {(one) => (
        <aside
          class={`sheet ${props.sheet}`}
          style={
            props.sheet === "beside" && props.width
              ? { width: `${props.width}px` }
              : undefined
          }
        >
          <Show when={props.sheet === "beside"}>
            <div class="wider" onPointerDown={widen} />
          </Show>
          <div class="sh">
            <b class={`ch ${TINT[one().mark]}`}>{MARK[one().mark]}</b>
            <h2>{one().path}</h2>
            <button
              class="ib grip"
              onClick={() => props.onExpand()}
              aria-label={props.sheet === "full" ? "Shrink" : "Expand"}
            >
              {props.sheet === "full" ? "⌄" : "⌃"}
            </button>
            <button
              class="ib grip"
              onClick={() => props.onClose()}
              aria-label="Close"
            >
              ×
            </button>
          </div>

          <div class="sm">
            <About
              definition={one()}
              because={because()}
              uses={uses()}
              step={props.step}
              review={props.review}
              onOpen={props.onOpen}
            />
          </div>

          <div
            class="sb"
            ref={(pane) => {
              sheet = pane;
              props.onPane(pane);
            }}
          >
            <Diff
              definition={one()}
              names={props.names}
              onOpen={props.onOpen}
            />
          </div>

          <div class="nav">
            <div class="pair">
              <button
                class="by"
                disabled={props.at === 0}
                onClick={() => props.onStep(-1)}
                aria-label="Back, without marking"
              >
                <span class="gl">back</span>
                <kbd>p</kbd>
              </button>
              <button
                class="by"
                disabled={props.at === props.review.steps.length - 1}
                onClick={() => props.onStep(1)}
                aria-label="Next, without marking"
              >
                <span class="gl">next</span>
                <kbd>n</kbd>
              </button>
              <button
                class={`pos${props.read ? " done" : ""}`}
                onClick={() => props.onToggle()}
                aria-label={
                  props.read ? "Viewed. Press to unmark" : "Not viewed yet"
                }
              >
                <span class="gl">
                  {props.read ? "✓ " : ""}
                  {props.at + 1}/{props.review.steps.length}
                </span>
                <kbd>m</kbd>
              </button>
              <button
                class="go"
                disabled={props.at === 0}
                onClick={() => props.onRead(-1)}
                aria-label="Viewed, and back"
              >
                <span class="gl">✓ back</span>
                <kbd>h</kbd>
              </button>
              <button
                class="go"
                onClick={() => props.onRead(1)}
                aria-label="Viewed, and next"
              >
                <span class="gl">✓ next</span>
                <kbd>l</kbd>
              </button>
            </div>
          </div>
        </aside>
      )}
    </Show>
  );
}

function About(props: {
  definition: Def;
  because: Identity[];
  uses: Identity[];
  review: Review;
  onOpen: (id: Identity) => void;
  step: Step | undefined;
}) {
  return (
    <>
      <p class="file">
        {props.definition.file}
        <span class="kind">{props.definition.kind}</span>
      </p>

      <Show when={props.because.length}>
        <p class="why">
          because:{" "}
          <Names
            ids={props.because}
            step={props.step}
            review={props.review}
            onOpen={props.onOpen}
          />
        </p>
      </Show>
      <Show when={props.uses.length}>
        <p class="why">
          uses:{" "}
          <Names
            ids={props.uses}
            step={props.step}
            review={props.review}
            onOpen={props.onOpen}
          />
        </p>
      </Show>
    </>
  );
}

function Names(props: {
  ids: Identity[];
  review: Review;
  step: Step | undefined;
  onOpen: (id: Identity) => void;
}) {
  const shown = () =>
    props.ids
      .map((id) => props.review.definitions.get(id))
      .filter((one): one is Def => one !== undefined)
      .map((definition) => ({
        definition,
        /* No step (viewing out of order) means nothing can be "not yet seen". */
        soon: (props.step?.on_faith || []).includes(definition.id),
      }));

  return (
    <For each={shown()}>
      {(one, index) => (
        <>
          <Show when={index() > 0}>, </Show>
          <b
            class={one.soon ? "soon" : ""}
            onClick={() => props.onOpen(one.definition.id)}
          >
            {one.definition.name}
            {one.soon ? " (not yet seen)" : ""}
          </b>
        </>
      )}
    </For>
  );
}

function Diff(props: {
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

  return (
    <pre
      class={`code${unchanged() ? " same" : ""}`}
      style={{ "--gutter": gutter() }}
    >
      <For each={lines()}>
        {(one, at) => (
          <span
            class={`ln ${one.mark === "+" ? "a" : one.mark === "−" ? "r" : ""}`}
          >
            <i>{one.mark === " " ? "" : one.mark}</i>
            <u>{one.line.at ?? ""}</u>
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
  const parts = (): Led[] =>
    props.pieces.flatMap((piece) =>
      piece.colour ? split(piece, props.names, props.here) : [piece],
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
            <span
              class="lnk"
              style={part.colour ? { color: part.colour } : undefined}
              onClick={() => props.onOpen(goes())}
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
