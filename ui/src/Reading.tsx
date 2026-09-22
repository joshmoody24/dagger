import { createEffect, createMemo, For, Show } from "solid-js";
import type { Box, Definition as Def, Identity, Review, Shown, Step } from "./dagger.ts";
import type { Painted } from "./colouring.ts";
import { MARK, TINT, broke } from "./digest.ts";
import { compare, focused } from "./diff.ts";
import { inside } from "./layout.ts";
import { stitch } from "./text.ts";
import { colouring, painted, readied, speaks } from "./colouring.ts";
import { dressing, wearing } from "./theme.ts";

/** Which definitions a name leads to, for the ones a reader can follow. */
type Names = Map<string, Identity>;

interface ReadingProps {
  review: Review;
  here: Identity | null;
  box: Box | null;
  step: Step | undefined;
  at: number;
  names: Names;
  read: boolean;
  viewed: Set<Identity>;
  sheet: string;
  width: number;
  onStep: (by: number) => void;
  onRead: (by: number) => void;
  onOpen: (id: Identity) => void;
  onToggle: () => void;
  onWiden: (width: number) => void;
  /** Handed the diff, so whoever owns the keyboard can scroll it. */
  onPane: (pane: HTMLDivElement) => void;
  onExpand: () => void;
  onClose: () => void;
}

/* The definition in front of the reader: what it is, why it's here, and how it changed. */
export function Reading(props: ReadingProps) {
  const definition = () => (props.here === null ? undefined : props.review.definitions.get(props.here));
  const leans = () =>
    props.review.edges
      .filter((edge) => edge.from === definition()?.id)
      .map((edge) => edge.to);

  const because = () => leans().filter((id) => broke(props.review, id));
  const uses = () => leans().filter((id) => !broke(props.review, id));

  let sheet: HTMLDivElement | undefined;

  /* Held while the pointer is down, so letting go anywhere — over the graph, outside the
   * window — ends the drag rather than leaving it stuck to the mouse. */
  const widen = (event: PointerEvent) => {
    const edge = event.currentTarget as HTMLElement;
    edge.setPointerCapture(event.pointerId);

    const move = (moved: PointerEvent) => props.onWiden(window.innerWidth - moved.clientX);
    const done = () => {
      edge.removeEventListener("pointermove", move);
      edge.removeEventListener("pointerup", done);
      edge.removeEventListener("pointercancel", done);
    };

    edge.addEventListener("pointermove", move);
    edge.addEventListener("pointerup", done);
    edge.addEventListener("pointercancel", done);
  };

  /* A long definition with one line changed near the bottom opens showing none of it. The
   * top of the sheet says what this is and why it's here, which is worth seeing, so the
   * first changed line is brought just under that rather than to the top of the pane. */
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
    <Show when={definition() || props.box}>
      <aside
        class={`sheet ${props.sheet}`}
        style={props.sheet === "beside" && props.width ? { width: `${props.width}px` } : undefined}
      >
        {/* Drag the edge to give the code more room. Only where it's a column — a drawer
          * covers the width already. */}
        <Show when={props.sheet === "beside"}>
          <div class="wider" onPointerDown={widen} />
        </Show>
        <div class="sh">
          <Show when={definition()}>
            {(one) => <b class={`ch ${TINT[one().mark]}`}>{MARK[one().mark]}</b>}
          </Show>
          <h2>{definition()?.path ?? props.box?.label ?? ""}</h2>
          <button class="ib grip" onClick={() => props.onExpand()} aria-label={props.sheet === "full" ? "Shrink" : "Expand"}>
            {props.sheet === "full" ? "⌄" : "⌃"}
          </button>
          <button class="ib grip" onClick={() => props.onClose()} aria-label="Close">×</button>
        </div>

        <div
          class="sb"
          ref={(pane) => {
            sheet = pane;
            props.onPane(pane);
          }}
        >
          <Show
            when={props.box}
            fallback={
              <Show when={definition()}>
                {(one) => (
                  <Definition
                    definition={one()}
                    because={because()}
                    uses={uses()}
                    step={props.step}
                    review={props.review}
                    names={props.names}
                    onOpen={props.onOpen}
                  />
                )}
              </Show>
            }
          >
            {(one) => (
              <Package
                box={one()}
                review={props.review}
                viewed={props.viewed}
                onOpen={props.onOpen}
              />
            )}
          </Show>
        </div>


        {/* Everything you can press sits together on the right, each wearing the key that
          * does the same job — a hint is worth more on the thing it applies to than in a
          * list somewhere else.
          *
          * Back and next are the same move in two directions, so they're said the same way
          * and coloured the same: both mark as viewed, because both mean you're finished
          * with what you're looking at. Moving without saying so is a quieter thing and
          * looks it. */}
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
              aria-label={props.read ? "Viewed. Press to unmark" : "Not viewed yet"}
            >
              <span class="gl">{props.read ? "✓ " : ""}{props.at + 1}/{props.review.steps.length}</span>
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
            <button class="go" onClick={() => props.onRead(1)} aria-label="Viewed, and next">
              <span class="gl">✓ next</span>
              <kbd>l</kbd>
            </button>
          </div>
        </div>
      </aside>
    </Show>
  );
}

/* One definition: what it is, why it's here, and how it changed. */
function Definition(props: {
  definition: Def;
  because: Identity[];
  uses: Identity[];
  review: Review;
  names: Names;
  onOpen: (id: Identity) => void;
  step: Step | undefined;
}) {
  return (
    <>
      {/* What a definition is comes from a fixed set, so it reads as a label rather than as
        * more of the sentence the path is. */}
      <p class="file">
        {props.definition.file}
        <span class="kind">{props.definition.kind}</span>
      </p>

      <Show when={props.because.length}>
        <p class="why">
          because: <Names ids={props.because} step={props.step} review={props.review} onOpen={props.onOpen} />
        </p>
      </Show>
      <Show when={props.uses.length}>
        <p class="why">
          uses: <Names ids={props.uses} step={props.step} review={props.review} onOpen={props.onOpen} />
        </p>
      </Show>

      <Diff definition={props.definition} names={props.names} onOpen={props.onOpen} />
    </>
  );
}

/* A box that stands for no definition of its own — a folder, a package. There's nothing to
 * read here, so this says what's inside and hands the reader to it. Picking one of its
 * contents on the reader's behalf would be answering a question nobody asked. */
function Package(props: { box: Box; review: Review; viewed: Set<Identity>; onOpen: (id: Identity) => void }) {
  const held = () => [...inside(props.box)].sort((a, b) => a.name.localeCompare(b.name));
  const changed = () => held().filter((one) => one.mark !== "affected" && one.mark !== "still");

  return (
    <>
      <p class="file">
        {props.box.key}
        <span class="kind">package</span>
      </p>
      <p class="why">
        {held().length} definition{held().length === 1 ? "" : "s"} here, {changed().length} changed
      </p>

      <ul class="held">
        <For each={held()}>
          {(one) => (
            <li>
              <button
                class={props.viewed.has(one.id) ? "done" : ""}
                onClick={() => props.onOpen(one.id)}
              >
                <b class={`ch ${TINT[one.mark]}`}>{MARK[one.mark]}</b>
                <span class="nm">{one.name}</span>
                <span class="at">{one.file}</span>
              </button>
            </li>
          )}
        </For>
      </ul>
    </>
  );
}

/* Names of other definitions, marked when the reader hasn't got to them yet — which is the
 * one thing they can't check for themselves. */
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
        /* Only the reading order can say whether something is being taken on faith. Looked
         * at on its own, out of order, there's no order for it to be out of. */
        soon: (props.step?.on_faith || []).includes(definition.id),
      }));

  return (
    <For each={shown()}>
      {(one, index) => (
        <>
          <Show when={index() > 0}>, </Show>
          <b class={one.soon ? "soon" : ""} onClick={() => props.onOpen(one.definition.id)}>
            {one.definition.name}{one.soon ? " (not yet seen)" : ""}
          </b>
        </>
      )}
    </For>
  );
}

function Diff(props: { definition: Def; names: Names; onOpen: (id: Identity) => void }) {
  /* Fetches the grammar for whatever this file is written in, once it's known. */
  createEffect(() => readied(speaks(props.definition.file), dressing(), wearing()));

  /* Held rather than worked out again on each read: every one of these is walked once per
   * line while the lines are drawn, and colouring a block is far too much work to repeat
   * a hundred times over the same block. */
  const lines = createMemo(() => {
    const [was, is] = [stitch(props.definition.before), stitch(props.definition.after)];
    if (!was && !is) return [];
    const all: Shown[] =
      !was
        ? is!.map((line) => ({ mark: "+" as const, line }))
        : !is
          ? was.map((line) => ({ mark: "−" as const, line }))
          : compare(was, is);
    return focused(all);
  });

  /* Unchanged means unchanged: a definition in the review because something it depends on
   * moved has no diff to show, only itself. */
  const unchanged = createMemo(() => lines().length > 0 && lines().every((one) => one.mark === " "));

  /* Coloured in one pass over the whole thing, so the grammar knows where it is. */
  const tinted = createMemo(() => {
    void colouring();
    return painted(lines().map((one) => one.line.text), speaks(props.definition.file), wearing());
  });

  /* Room for the largest line number this diff holds, and no more. */
  const gutter = () => {
    const most = Math.max(0, ...lines().map((one) => one.line.at ?? 0));
    return `${Math.max(3, String(most).length)}ch`;
  };

  return (
    <pre class={`code${unchanged() ? " same" : ""}`} style={{ "--gutter": gutter() }}>
      <For each={lines()}>
        {(one, at) => (
          <span class={`ln ${one.mark === "+" ? "a" : one.mark === "−" ? "r" : ""}`}>
            <i>{one.mark === " " ? "" : one.mark}</i>
            {/* Where it is in the file, so a reader can say "line 31" and be understood.
              * A gap stands between two pieces and is nowhere in the file, so it has no
              * number to show. */}
            <u>{one.line.at ?? ""}</u>
            <Show when={one.line.at !== null} fallback={<span class="gap">…</span>}>
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

/* Comments and strings step back, and a name that belongs to something else in this review
 * becomes a way to get there. That last part is why this isn't a highlighting library: no
 * library knows which words in this code the reader is about to meet. */
function Code(props: {
  pieces: Painted[];
  names: Names;
  here: Identity;
  onOpen: (id: Identity) => void;
}) {
  /* Coloured by the grammar, then read again for names that go somewhere. A highlighter
   * can't know which words in this code are definitions the reader is about to meet, and
   * that's the one thing worth more than the colour. */
  const parts = (): Led[] =>
    props.pieces.flatMap((piece) =>
      piece.colour ? split(piece, props.names, props.here) : [piece],
    );

  return (
    <For each={parts()}>
      {(part) => (
        <Show
          when={part.goes}
          fallback={<span style={part.colour ? { color: part.colour } : undefined}>{part.text}</span>}
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

/** A coloured piece, and where its text leads if it names something else in the review. */
type Led = Painted & { goes?: Identity };

/* A coloured piece, cut around any names in it that lead somewhere else. */
function split(piece: Painted, names: Names, here: Identity): Led[] {
  const goes = names.get(piece.text.trim());
  if (goes !== undefined && goes !== here && piece.text.trim() === piece.text) {
    return [{ ...piece, goes }];
  }
  return [piece];
}
