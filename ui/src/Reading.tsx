import { createEffect, For, Show } from "solid-js";
import { MARK, TINT, broke, compare, stitch, tokens } from "./review.ts";

/* The definition in front of the reader: what it is, why it's here, and how it changed. */
export function Reading(props) {
  const definition = () => props.review.definitions.get(props.here);
  const leans = () =>
    props.review.edges.filter((edge) => edge.from === definition().id).map((edge) => edge.to);

  const because = () => leans().filter((id) => broke(props.review, id));
  const uses = () => leans().filter((id) => !broke(props.review, id));

  let sheet: HTMLDivElement | undefined;

  /* A long definition with one line changed near the bottom opens showing none of it. The
   * top of the sheet says what this is and why it's here, which is worth seeing, so the
   * first changed line is brought just under that rather than to the top of the pane. */
  createEffect(() => {
    const here = definition();
    if (!here || !sheet) return;

    queueMicrotask(() => {
      const changed = sheet?.querySelector(".ln.a, .ln.r");
      if (!changed) {
        if (sheet) sheet.scrollTop = 0;
        return;
      }
      const above = sheet!.getBoundingClientRect().top;
      const onto = changed.getBoundingClientRect().top;
      sheet!.scrollTop += onto - above - 12;
    });
  });

  return (
    <Show when={definition()}>
      <aside class={`sheet ${props.sheet}`}>
        <div class="sh">
          <b class={`ch ${TINT[definition().mark]}`}>{MARK[definition().mark]}</b>
          <h2>{definition().path}</h2>
          <button class="ib grip" onClick={props.onExpand} aria-label={props.sheet === "full" ? "Shrink" : "Expand"}>
            {props.sheet === "full" ? "⌄" : "⌃"}
          </button>
          <button class="ib grip" onClick={props.onClose} aria-label="Close">×</button>
        </div>

        <div class="sb" ref={sheet}>
          {/* What a definition is comes from a fixed set, so it reads as a label rather than
            * as more of the sentence the path is. */}
          <p class="file">
            {definition().file}
            <span class="kind">{definition().kind}</span>
          </p>

          <Show when={because().length}>
            <p class="why">
              because: <Names ids={because()} step={props.step} review={props.review} onOpen={props.onOpen} />
            </p>
          </Show>
          <Show when={uses().length}>
            <p class="why">
              uses: <Names ids={uses()} step={props.step} review={props.review} onOpen={props.onOpen} />
            </p>
          </Show>

          <Diff definition={definition()} names={props.names} onOpen={props.onOpen} />
        </div>

        {/* Everything you can press sits together on the right, each wearing the key that
          * does the same job — a hint is worth more on the thing it applies to than in a
          * list somewhere else. Moving on and marking as viewed are one button, because
          * they're one thing a reader does; the count doubles as the way to take it back. */}
        <div class="nav">
          <div class="pair">
            <button
              class="nb"
              disabled={props.at === 0}
              onClick={() => props.onStep(-1)}
              aria-label="Back"
            >
              <span class="gl">‹</span>
              <kbd>k</kbd>
            </button>
            <button
              class="nb"
              disabled={props.at === props.review.steps.length - 1}
              onClick={() => props.onStep(1)}
              aria-label="Skip ahead"
            >
              <span class="gl">›</span>
              <kbd>n</kbd>
            </button>
            <button
              class={`pos${props.read ? " done" : ""}`}
              onClick={props.onToggle}
              aria-label={props.read ? "Viewed. Press to unmark" : "Not viewed yet"}
            >
              <span class="gl">{props.read ? "✓ " : ""}{props.at + 1}/{props.review.steps.length}</span>
              <kbd>m</kbd>
            </button>
            <button class="go" onClick={props.onRead}>
              <span class="gl">✓ next</span>
              <kbd>j</kbd>
            </button>
          </div>
        </div>
      </aside>
    </Show>
  );
}

/* Names of other definitions, marked when the reader hasn't got to them yet — which is the
 * one thing they can't check for themselves. */
function Names(props) {
  const shown = () =>
    props.ids
      .map((id) => props.review.definitions.get(id))
      .filter(Boolean)
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

function Diff(props) {
  const before = () => stitch(props.definition.before);
  const after = () => stitch(props.definition.after);

  const lines = () => {
    const [was, is] = [before(), after()];
    if (was === null && is === null) return [];
    if (was === is) return is.split("\n").map((line) => [" ", line]);
    if (was === null) return is.split("\n").map((line) => ["+", line]);
    if (is === null) return was.split("\n").map((line) => ["−", line]);
    return compare(was, is);
  };

  return (
    <pre class={`code${before() === after() ? " same" : ""}`}>
      <For each={lines()}>
        {([mark, line]) => (
          <Show when={line !== "…"} fallback={<span class="ln gap">…</span>}>
            <span class={`ln ${mark === "+" ? "a" : mark === "−" ? "r" : ""}`}>
              <i>{mark === " " ? "" : mark}</i>
              <Code line={line} names={props.names} here={props.definition.id} onOpen={props.onOpen} />
            </span>
          </Show>
        )}
      </For>
    </pre>
  );
}

/* Comments and strings step back, and a name that belongs to something else in this review
 * becomes a way to get there. That last part is why this isn't a highlighting library: no
 * library knows which words in this code the reader is about to meet. */
function Code(props) {
  const parts = () =>
    tokens(props.line).map((token) => ({
      ...token,
      goes: token.kind === "name" ? props.names.get(token.text) : undefined,
    }));

  return (
    <For each={parts()}>
      {(part) => (
        <Show
          when={part.goes !== undefined && part.goes !== props.here}
          fallback={<span class={part.kind === "quiet" ? "tq" : ""}>{part.text}</span>}
        >
          <span class="lnk" onClick={() => props.onOpen(part.goes)}>{part.text}</span>
        </Show>
      )}
    </For>
  );
}
