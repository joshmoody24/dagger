import { createMemo, createSignal, For, onCleanup, onMount, Show } from "solid-js";
import { Graph } from "./Graph.jsx";
import { Reading } from "./Reading.jsx";
import { digest, layout, namesIn } from "./review.js";

export function App(props) {
  const review = createMemo(() => digest(props.raw));
  const laid = createMemo(() => layout(review()));
  const names = createMemo(() => namesIn(review()));

  const [at, setAt] = createSignal(0);
  const [read, setRead] = createSignal(new Set());
  const [worriesOpen, setWorriesOpen] = createSignal(false);

  /* Narrow enough that the sheet has to cover the graph rather than sit beside it. The
   * sheet is then a drawer with three heights, and the graph is only worth looking at while
   * it's out of the way — so it starts shut. Beside the graph there's nothing to shut. */
  const [wide, setWide] = createSignal(true);
  const [sheet, setSheet] = createSignal("closed");

  onMount(() => {
    const beside = window.matchMedia("(min-width: 900px)");
    const settle = () => setWide(beside.matches);
    settle();
    beside.addEventListener("change", settle);
    onCleanup(() => beside.removeEventListener("change", settle));
  });

  const showing = () => wide() || sheet() !== "closed";
  const open = () => !wide() && sheet() === "closed" && setSheet("half");
  const shut = () => !wide() && setSheet("closed");

  const steps = () => review().steps;
  const here = () => (steps()[at()] || {}).definition;

  const step = (by) => {
    setAt((was) => Math.min(Math.max(was + by, 0), steps().length - 1));
    open();
  };
  const markRead = () => setRead((was) => new Set(was).add(here()));
  const toggleRead = () =>
    setRead((was) => {
      const now = new Set(was);
      if (!now.delete(here())) now.add(here());
      return now;
    });
  const goTo = (id) => {
    const found = steps().findIndex((step) => step.definition === id);
    if (found >= 0) setAt(found);
    open();
  };

  /* The same keys as the command line, because the point of both is to read a change
   * without taking a hand off the keyboard. Nothing here needs anything focused. */
  const keys = {
    j: () => { markRead(); step(1); },
    n: () => step(1),
    ArrowDown: () => step(1),
    " ": () => step(1),
    k: () => step(-1),
    ArrowUp: () => step(-1),
    m: () => toggleRead(),
    g: () => setAt(0),
    G: () => setAt(steps().length - 1),
    Escape: () => (worriesOpen() ? setWorriesOpen(false) : shut()),
  };

  const onKey = (event) => {
    if (event.metaKey || event.ctrlKey || event.altKey) return;
    const pressed = keys[event.key];
    if (!pressed) return;
    pressed();
    event.preventDefault();
  };

  document.addEventListener("keydown", onKey);
  onCleanup(() => document.removeEventListener("keydown", onKey));

  return (
    <>
      <header>
        <div class="t">
          <span class="prog">{at() + 1} of {steps().length}</span>
          <span>{steps().length} to read, grouped by {review().grouping || "file"}</span>
        </div>
        <div class="d">
          {review().cost.peak_open} definitions in mind at once
          {" · "}{review().cost.taken_on_faith} out of order
          {" · "}{review().cost.jumps} module jumps
        </div>
        <Legend />
        {/* Only there when there's something to say. Nothing went wrong most of the time,
          * and a permanent badge reading nought is just something to learn to ignore. */}
        <Show when={review().worries.length}>
          <button
            class="worry"
            onClick={() => setWorriesOpen((was) => !was)}
            title={`${review().worries.length} dagger couldn't work out`}
            aria-label={`${review().worries.length} dagger couldn't work out`}
          >
            ⚠
          </button>
        </Show>
      </header>

      <main class={showing() ? `has-sheet ${sheet()}` : ""}>
        {/* Tapping past the graph puts the drawer away, the way tapping off any sheet does. */}
        <div class="stage" onClick={(event) => !event.target.closest(".nd, .box text") && shut()}>
          <Graph review={review()} laid={laid()} here={here()} read={read()} onOpen={goTo} />
        </div>
        <Reading
          review={review()}
          step={steps()[at()]}
          at={at()}
          onStep={step}
          onRead={() => { markRead(); step(1); }}
          onOpen={goTo}
          names={names()}
          read={read().has(here())}
          onToggle={toggleRead}
          sheet={wide() ? "beside" : sheet()}
          onExpand={() => setSheet((was) => (was === "full" ? "half" : "full"))}
          onClose={shut}
        />
      </main>

      <Show when={worriesOpen()}>
        <aside class="notes">
          <button class="ib" onClick={() => setWorriesOpen(false)} aria-label="Close">×</button>
          <h2>What dagger couldn't work out</h2>
          <ul>
            <For each={review().worries}>{(worry) => <li>{worry}</li>}</For>
          </ul>
        </aside>
      </Show>
    </>
  );
}

function Legend() {
  const marks = [
    ["add", "+", "new"],
    ["del", "−", "gone"],
    ["chg", "!", "callers affected"],
    ["chg", "~", "body"],
    ["chg", '"', "docs"],
    ["aff", "≈", "affected by something else"],
  ];
  return (
    <div class="lg">
      <For each={marks}>
        {([tint, mark, said]) => (
          <span><b class={`ch ${tint}`}>{mark}</b>{said}</span>
        )}
      </For>
    </div>
  );
}
