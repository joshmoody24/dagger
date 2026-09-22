import { createEffect, createMemo, createSignal, For, type JSXElement, onCleanup, onMount, Show } from "solid-js";
import { ChartColumn, Info, TriangleAlert, Waves } from "lucide-solid";
import { Graph } from "./Graph.tsx";
import { Reading } from "./Reading.tsx";
import type { Box, Identity, Raw } from "./dagger.ts";
import { digest, layout, namesIn } from "./review.ts";
import { next as another, wear, wearing } from "./theme.ts";

/* How fast j and k scroll, in pixels a second. About sixty lines, which crosses a long
 * definition without waiting on it and still reads on the way past. */
const SPEED = 1240;

export function App(props: { raw: Raw }) {
  const whole = createMemo(() => digest(props.raw));

  /* How far out to show what a change reached.
   *
   * Starts at none. What changed is the review; what a change reached is a second and
   * larger question, and one worth asking on purpose rather than being handed. Never goes
   * further than the reading went — whoever ran dagger said how far to follow with
   * --ripples, and a page offering more than that would be offering to show nothing. */
  const [ripples, setRipples] = createSignal(0);
  const further = () => setRipples((was) => (was >= whole().ripples ? 0 : was + 1));

  const review = createMemo(() => {
    const all = whole();
    const far = ripples();
    if (far >= whole().ripples) return all;

    const gone = new Set(
      [...all.definitions.values()]
        .filter((one) => one.mark === "affected" && one.away > far)
        .map((one) => one.id),
    );
    if (!gone.size) return all;

    /* A module that's only here to hold something goes when that something does. These are
     * the context tier — drawn, never read — and a file earns one by having members in the
     * review. Take the members away and what's left is an empty box labelled after a file
     * nothing in the reading mentions. */
    const holds = new Set(
      [...all.definitions.values()]
        .filter((one) => one.kind !== "module" && !gone.has(one.id))
        .map((one) => one.file),
    );
    for (const one of all.definitions.values()) {
      if (one.kind === "module" && one.mark === "still" && !holds.has(one.file)) gone.add(one.id);
    }

    return {
      ...all,
      definitions: new Map([...all.definitions].filter(([id]) => !gone.has(id))),
      steps: all.steps.filter((step) => !gone.has(step.definition)),
      edges: all.edges.filter((edge) => !gone.has(edge.from) && !gone.has(edge.to)),
    };
  });

  const laid = createMemo(() => layout(review()));
  const names = createMemo(() => namesIn(review()));

  const [at, setAt] = createSignal(0);
  const [read, setRead] = createSignal<Set<Identity>>(new Set());
  const [worriesOpen, setWorriesOpen] = createSignal(false);
  const [costOpen, setCostOpen] = createSignal(false);

  /* Narrow enough that the sheet has to cover the graph rather than sit beside it. The
   * sheet is then a drawer with three heights, and the graph is only worth looking at while
   * it's out of the way — so it starts shut. Beside the graph there's nothing to shut. */
  /* The diff, so the keys that scroll it have something to scroll. It belongs to the
   * sheet and is handed back rather than reached for, since a page that queries its own
   * markup has two descriptions of it. */
  let pane: HTMLDivElement | undefined;

  /* Scrolling the diff while a key is held.
   *
   * Driven by holding rather than by pressing, because a keyboard repeats on its own
   * schedule: one press, half a second of nothing, then a stream of them. Moving on each
   * repeat inherits that — it starts, stops, and starts again — however smooth each step
   * is. So a key going down means start, a key coming up means stop, and in between this
   * moves at one speed regardless of what the keyboard is doing.
   */
  let going = 0;
  let rolling = 0;
  let last = 0;

  const roll = (now: number) => {
    const sheet = pane;
    if (!sheet || !going) {
      rolling = 0;
      return;
    }
    // By the clock, not by the frame, so it travels the same on any screen.
    const since = last ? Math.min(now - last, 100) : 16;
    last = now;
    sheet.scrollTop += going * SPEED * (since / 1000);
    rolling = requestAnimationFrame(roll);
  };

  const scroll = (way: number) => {
    going = way;
    last = 0;
    if (!rolling) rolling = requestAnimationFrame(roll);
  };
  const settle = (way: number) => {
    if (going === way) going = 0;
  };
  onCleanup(() => rolling && cancelAnimationFrame(rolling));

  const [wide, setWide] = createSignal(true);
  const [sheet, setSheet] = createSignal("closed");

  onMount(() => {
    const room = window.matchMedia("(min-width: 900px)");
    const settle = () => setWide(room.matches);
    settle();
    room.addEventListener("change", settle);
    onCleanup(() => room.removeEventListener("change", settle));
  });

  const hiding = () => review().warnings.filter((one) => one.impact === "incomplete");
  const weaker = () => review().warnings.filter((one) => one.impact === "degraded");
  const said = () =>
    hiding().length
      ? `${hiding().length} this review might not be showing`
      : `${weaker().length} worked out a weaker way`;

  const spread = (one: Box): Box[] => [one, ...one.boxes.flatMap(spread)];
  /* Beside the graph it's a column you can put away and drag wider; over the graph it's a
   * drawer. Either way, doing anything at all brings it back — you might skip past
   * something you hadn't read, but you asked to go there, and a tool that argues about
   * that is worse than one that does as it's told. */
  const [beside, setBeside] = createSignal(true);
  const [width, setWidth] = createSignal(0);

  const showing = () => (wide() ? beside() : sheet() !== "closed");
  const open = () => {
    setBeside(true);
    if (!wide() && sheet() === "closed") setSheet("half");
  };
  const shut = () => (wide() ? setBeside(false) : setSheet("closed"));
  const facing = () => (wide() ? (beside() ? "beside" : "away") : sheet());

  /* Putting the ripples away can leave the reading past its end. */
  const steps = () => review().steps;
  createEffect(() => setAt((was) => Math.min(was, Math.max(steps().length - 1, 0))));

  /* What's being looked at, which isn't always a step.
   *
   * Most of the page is the reading order, and `at` is where in it you are. But a module
   * that didn't itself change is on the page without being in that order — it's the box
   * around things that did — and picking one has to show it rather than quietly show
   * something else. So the reading has a position, and looking has a subject, and stepping
   * puts the two back together. */
  const [aside, setAside] = createSignal<Identity | null>(null);
  /* A box that stands for no definition — a folder full of them — can still be looked at,
   * and looking at it shows the box rather than something inside it. */
  const [box, setBox] = createSignal<string | null>(null);
  const here = () => (box() ? null : aside() ?? steps()[at()]?.definition ?? null);
  const next = () => steps()[at() + 1]?.definition ?? null;
  const stepping = () => aside() === null;

  const step = (by: number) => {
    setAside(null);
    setBox(null);
    setAt((was) => Math.min(Math.max(was + by, 0), steps().length - 1));
    open();
  };
  const markRead = () => {
    const id = here();
    if (id !== null) setRead((was) => new Set(was).add(id));
  };
  const toggleRead = () =>
    setRead((was) => {
      const id = here();
      if (id === null) return was;
      const now = new Set(was);
      if (!now.delete(id)) now.add(id);
      return now;
    });
  const goTo = (id: Identity) => {
    setBox(null);
    const found = steps().findIndex((step) => step.definition === id);
    if (found >= 0) {
      setAside(null);
      setAt(found);
    } else {
      setAside(id);
    }
    open();
  };

  const goToBox = (key: string) => {
    setAside(null);
    setBox(key);
    open();
  };

  /* The same keys as the command line, because the point of both is to read a change
   * without taking a hand off the keyboard. Nothing here needs anything focused. */
  const keys = {
    /* The way through a review: done with this one, on to the next. Back is the same move
     * in the other direction — you're as finished with it either way, and a way forward
     * that marks and a way back that doesn't is two different ideas wearing one pair of
     * keys.
     *
     * Sideways, because that's what moving between definitions is. Up and down belong to
     * the thing you're reading. */
    l: () => { markRead(); step(1); },
    h: () => { markRead(); step(-1); },
    /* Through the one in front of you, which is where up and down mean what they say.
     * A few lines at a time: one is too slow to hold, a screenful loses your place. */
    j: () => scroll(1),
    k: () => scroll(-1),
    /* The way around it, for when you want to look without saying you've looked. */
    n: () => step(1),
    p: () => step(-1),
    ArrowDown: () => step(1),
    ArrowUp: () => step(-1),
    " ": () => step(1),
    m: () => { toggleRead(); open(); },
    /* Away, and nothing left behind to say so. A strip down the side saying "there's a
     * thing here" is the thing, taking up room. */
    d: () => (showing() ? shut() : open()),
    r: () => further(),
    /* Trying colours on. Every one is somebody's editor, so the question is which, and the
     * only way to answer it is to look. */
    t: () => void wear(another(wearing())),
    g: () => setAt(0),
    G: () => setAt(steps().length - 1),
    Escape: () => {
      if (!worriesOpen() && !costOpen()) {
        shut();
        return;
      }
      setWorriesOpen(false);
      setCostOpen(false);
    },
  };

  const onKey = (event: KeyboardEvent) => {
    if (event.metaKey || event.ctrlKey || event.altKey) return;
    const pressed = keys[event.key as keyof typeof keys];
    if (!pressed) return;
    /* The keyboard repeating a held key says nothing new: whatever it asked for is
     * already happening, and doing it again is what made scrolling stutter. */
    if (!event.repeat) void pressed();
    event.preventDefault();
  };

  /* Letting go stops what holding started. Losing the window counts as letting go of
   * everything, since no key will ever come up to say otherwise. */
  const onLift = (event: KeyboardEvent) => {
    if (event.key === "j") settle(1);
    if (event.key === "k") settle(-1);
  };
  const onLeave = () => {
    settle(1);
    settle(-1);
  };

  document.addEventListener("keyup", onLift);
  window.addEventListener("blur", onLeave);
  onCleanup(() => {
    document.removeEventListener("keyup", onLift);
    window.removeEventListener("blur", onLeave);
  });
  document.addEventListener("keydown", onKey);
  onCleanup(() => document.removeEventListener("keydown", onKey));

  return (
    <>
      <header>
        <div class="t">
          <span class="prog">{at() + 1}/{steps().length}</span>
          {/* Only there when there's something to say, and only a warning when something
            * might be missing. A review worked out a weaker way is worth knowing about and
            * isn't worth alarm — told as alarm, it teaches you to ignore the alarm. */}
          <Show when={review().warnings.length}>
            <button
              class={`worry${hiding().length ? " bad" : ""}`}
              onClick={() => { setCostOpen(false); setWorriesOpen((was) => !was); }}
              title={said()}
              aria-label={said()}
            >
              <Show when={hiding().length} fallback={<Info size={17} />}>
                <TriangleAlert size={17} />
              </Show>
            </button>
          </Show>

          {/* What the reading cost, kept behind the same corner as everything else that's
            * worth a look but isn't worth a line of the page. It doesn't change while you
            * read, so it doesn't need to sit there while you do. */}
          <button
            class="worry"
            onClick={() => { setWorriesOpen(false); setCostOpen((was) => !was); }}
            title="Cognitive load metrics"
            aria-label="Cognitive load metrics"
          >
            <ChartColumn size={17} />
          </button>

          {/* How far out what the change reached is shown. The number is the point — a
            * reader turning it down wants to know what they've turned it down to — so it
            * sits beside the mark rather than hiding in a tooltip. */}
          <Show when={whole().ripples > 0 && [...whole().definitions.values()].some((one) => one.away > 0)}>
            <button
              class={`worry steps${ripples() ? " on" : ""}`}
              onClick={further}
              title={`Ripples: showing what the change reached ${ripples()} of ${whole().ripples} steps out (r)`}
              aria-label={`Ripples: ${ripples()} of ${whole().ripples} steps out`}
            >
              <Waves size={17} />
              <b>{ripples()}</b>
            </button>
          </Show>
        </div>
      </header>

      <main class={showing() ? `has-sheet ${sheet()}` : ""}>
        <div class="stage">
          <Graph
            review={review()}
            laid={laid()}
            here={here()}
            next={next()}
            read={read()}
            box={box()}
            onOpen={goTo}
            onOpenBox={goToBox}
          />
        </div>
        <Reading
          review={review()}
          here={here()}
          box={laid().boxes.flatMap(spread).find((one) => one.key === box()) ?? null}
          step={stepping() ? steps()[at()] : undefined}
          at={at()}
          onStep={step}
          onRead={(by: number) => { markRead(); step(by); }}
          onOpen={goTo}
          names={names()}
          read={here() !== null && read().has(here()!)}
          viewed={read()}
          onToggle={toggleRead}
          sheet={facing()}
          width={width()}
          onPane={(el: HTMLDivElement) => (pane = el)}
          onWiden={setWidth}
          onExpand={() => setSheet((was) => (was === "full" ? "half" : "full"))}
          onClose={shut}
        />
      </main>

      {/* A dialog rather than a floating box: the browser puts it above everything, traps
        * the keyboard inside it, closes it on Escape and dims what's behind — all of which
        * would otherwise be ours to get wrong. */}
      <Panel open={costOpen()} onClose={() => setCostOpen(false)} title="Cognitive load metrics">
        <ul>
          <li>{review().cost.peak_open} definitions in mind at once</li>
          <li>{review().cost.taken_on_faith} definitions out of order</li>
          <li>{review().cost.jumps} {review().grouping || "module"} jumps</li>
          <li>{review().steps.length} definitions to read</li>
        </ul>
      </Panel>

      <Panel
        open={worriesOpen()}
        onClose={() => setWorriesOpen(false)}
        title={hiding().length ? "This review might not be showing" : "Worked out a weaker way"}
      >
        <Show when={hiding().length}>
          <ul>
            <For each={hiding()}>{(one) => <li>{one.message}</li>}</For>
          </ul>
        </Show>
        <Show when={weaker().length}>
          <Show when={hiding().length}>
            <h3>Worked out a weaker way</h3>
          </Show>
          <ul>
            <For each={weaker()}>{(one) => <li>{one.message}</li>}</For>
          </ul>
        </Show>
      </Panel>

    </>
  );
}

/* Anything the page wants to say beside itself. Native, so Escape and the click outside are
 * the browser's job rather than ours to reimplement badly. */
function Panel(props: { open: boolean; onClose: () => void; title: string; children: JSXElement }) {
  let box: HTMLDialogElement | undefined;

  createEffect(() => {
    if (!box) return;
    if (props.open && !box.open) box.showModal();
    if (!props.open && box.open) box.close();
  });

  return (
    <dialog
      class="panel"
      ref={box}
      onClose={() => props.onClose()}
      onClick={(event) => event.target === box && props.onClose()}
    >
      <h2>{props.title}</h2>
      {props.children}
    </dialog>
  );
}
