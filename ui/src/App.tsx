import {
  createEffect,
  createMemo,
  createSignal,
  For,
  type JSXElement,
  onCleanup,
  onMount,
  Show,
} from "solid-js";
import {
  ChartColumn,
  Info,
  TriangleAlert,
  Waves,
  Workflow,
} from "lucide-solid";
import { Graph } from "./Graph.tsx";
import { Reading } from "./Reading.tsx";
import type { Identity, Raw } from "./dagger.ts";
import { digest } from "./digest.ts";
import { layout } from "./layout.ts";
import { namesIn } from "./text.ts";
import { next as another, wear, wearing } from "./theme.ts";

/* j/k scroll speed in px/s. About sixty lines: fast enough to cross a long definition,
 * slow enough to still read on the way past. */
const SPEED = 1240;

export function App(props: { raw: Raw }) {
  const whole = createMemo(() => digest(props.raw));

  /* Starts at 0 so affected-only definitions are opt-in. Capped at what dagger was run
   * with (--ripples), since nothing beyond that exists in the data. */
  const [ripples, setRipples] = createSignal(0);
  const further = () =>
    setRipples((was) => (was >= whole().ripples ? 0 : was + 1));

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

    return {
      ...all,
      definitions: new Map(
        [...all.definitions].filter(([id]) => !gone.has(id)),
      ),
      steps: all.steps.filter((step) => !gone.has(step.definition)),
      edges: all.edges.filter(
        (edge) => !gone.has(edge.from) && !gone.has(edge.to),
      ),
    };
  });

  const laid = createMemo(() => layout(review()));
  const names = createMemo(() => namesIn(review()));

  const [at, setAt] = createSignal(0);
  const [read, setRead] = createSignal<Set<Identity>>(new Set());
  const [worriesOpen, setWorriesOpen] = createSignal(false);
  const [costOpen, setCostOpen] = createSignal(false);
  const [showNext, setShowNext] = createSignal(false);

  /* The diff pane, handed up by Reading so j/k can scroll it. */
  let pane: HTMLDivElement | undefined;

  /* j/k scroll via rAF between keydown and keyup rather than per key repeat, because
   * repeat timing (delay, then bursts) makes the scroll stutter. */
  let going = 0;
  let rolling = 0;
  let last = 0;

  const roll = (now: number) => {
    const sheet = pane;
    if (!sheet || !going) {
      rolling = 0;
      return;
    }
    // Time-based so speed is the same at any refresh rate.
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

  const hiding = () =>
    review().warnings.filter((one) => one.impact === "incomplete");
  const weaker = () =>
    review().warnings.filter((one) => one.impact === "degraded");
  const said = () =>
    hiding().length
      ? `${hiding().length} this review might not be showing`
      : `${weaker().length} worked out a weaker way`;

  /* Wide: a closable, resizable column. Narrow: a drawer. Any navigation reopens it. */
  const [beside, setBeside] = createSignal(true);
  const [width, setWidth] = createSignal(0);

  const showing = () => (wide() ? beside() : sheet() !== "closed");
  const open = () => {
    setBeside(true);
    if (!wide() && sheet() === "closed") setSheet("half");
  };
  const shut = () => (wide() ? setBeside(false) : setSheet("closed"));
  const facing = () => (wide() ? (beside() ? "beside" : "away") : sheet());

  /* Turning ripples down can shrink steps() below `at`. */
  const steps = () => review().steps;
  createEffect(() =>
    setAt((was) => Math.min(was, Math.max(steps().length - 1, 0))),
  );

  /* `aside` is a definition being viewed that isn't in the reading order (e.g. an
   * unchanged module). Stepping clears it and returns to `at`. */
  const [aside, setAside] = createSignal<Identity | null>(null);
  const here = () => aside() ?? steps()[at()]?.definition ?? null;
  const next = () => steps()[at() + 1]?.definition ?? null;
  const stepping = () => aside() === null;

  const step = (by: number) => {
    setAside(null);
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
    const found = steps().findIndex((step) => step.definition === id);
    if (found >= 0) {
      setAside(null);
      setAt(found);
    } else {
      setAside(id);
    }
    open();
  };

  /* Same keys as the CLI. Global, so nothing needs focus. */
  const keys = {
    /* h/l both mark as read: going back means you're done with the current one too. */
    l: () => {
      markRead();
      step(1);
    },
    h: () => {
      markRead();
      step(-1);
    },
    j: () => scroll(1),
    k: () => scroll(-1),
    /* n/p step without marking as read. */
    n: () => step(1),
    p: () => step(-1),
    ArrowDown: () => step(1),
    ArrowUp: () => step(-1),
    " ": () => step(1),
    m: () => {
      toggleRead();
      open();
    },
    d: () => (showing() ? shut() : open()),
    r: () => further(),
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
    /* j/k run from keydown to keyup, so ignore their repeats. Other keys should repeat. */
    const held = event.key === "j" || event.key === "k";
    if (!(held && event.repeat)) void pressed();
    event.preventDefault();
  };

  /* Window blur stops scrolling too, since the keyup will never arrive. */
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
        <Show when={whole().title}>
          <h1 class="rt">{whole().title}</h1>
        </Show>
        <div class="t">
          <span class="prog">
            {at() + 1}/{steps().length}
          </span>
          {/* Warning icon only for "incomplete"; "degraded" is info so alarms stay meaningful. */}
          <Show when={review().warnings.length}>
            <button
              class={`worry${hiding().length ? " bad" : ""}`}
              onClick={() => {
                setCostOpen(false);
                setWorriesOpen((was) => !was);
              }}
              title={said()}
              aria-label={said()}
            >
              <Show when={hiding().length} fallback={<Info size={17} />}>
                <TriangleAlert size={17} />
              </Show>
            </button>
          </Show>

          <button
            class="worry"
            onClick={() => {
              setWorriesOpen(false);
              setCostOpen((was) => !was);
            }}
            title="Cognitive load metrics"
            aria-label="Cognitive load metrics"
          >
            <ChartColumn size={17} />
          </button>

          <button
            class={`worry${showNext() ? " on" : ""}`}
            onClick={() => setShowNext((was) => !was)}
            title="Show where the reading goes next"
            aria-label="Show where the reading goes next"
            aria-pressed={showNext()}
          >
            <Workflow size={17} />
          </button>

          <Show
            when={
              whole().ripples > 0 &&
              [...whole().definitions.values()].some((one) => one.away > 0)
            }
          >
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
            next={showNext() ? next() : null}
            read={read()}
            onOpen={goTo}
          />
        </div>
        <Reading
          review={review()}
          here={here()}
          step={stepping() ? steps()[at()] : undefined}
          at={at()}
          onStep={step}
          onRead={(by: number) => {
            markRead();
            step(by);
          }}
          onOpen={goTo}
          names={names()}
          read={here() !== null && read().has(here())}
          onToggle={toggleRead}
          sheet={facing()}
          width={width()}
          onPane={(el: HTMLDivElement) => (pane = el)}
          onWiden={setWidth}
          onExpand={() => setSheet((was) => (was === "full" ? "half" : "full"))}
          onClose={shut}
        />
      </main>

      <Panel
        open={costOpen()}
        onClose={() => setCostOpen(false)}
        title="Cognitive load metrics"
      >
        <ul>
          <li>{review().cost.peak_open} definitions in mind at once</li>
          <li>{review().cost.taken_on_faith} definitions out of order</li>
          <li>
            {review().cost.jumps} {review().grouping || "module"} jumps
          </li>
          <li>{review().steps.length} definitions to read</li>
        </ul>
      </Panel>

      <Panel
        open={worriesOpen()}
        onClose={() => setWorriesOpen(false)}
        title={
          hiding().length
            ? "This review might not be showing"
            : "Worked out a weaker way"
        }
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

/* Native <dialog> so focus trapping, Escape and the backdrop come for free. */
function Panel(props: {
  open: boolean;
  onClose: () => void;
  title: string;
  children: JSXElement;
}) {
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
