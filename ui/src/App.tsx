import { createEffect, createMemo, createSignal } from "solid-js";
import { Graph } from "./Graph.tsx";
import { Reading, type Facing } from "./Reading.tsx";
import { Toolbar } from "./Toolbar.tsx";
import { CostPanel, type Opened, WarningsPanel } from "./Panels.tsx";
import type { Identity, Raw } from "./dagger.ts";
import { digest } from "./digest.ts";
import { matches } from "./graph/scene.ts";
import { useReviewKeys } from "./keys.ts";
import { layout } from "./layout.ts";
import { createMediaQuery, WIDE } from "./media.ts";
import { narrowed } from "./narrow.ts";
import { compared } from "./progress.ts";
import { namesIn } from "./text.ts";
import { next as another, wear, wearing } from "./theme.ts";

/** Where the reader wants the sheet: a column or a drawer, as the screen allows. */
type Pane = "away" | "beside" | "half" | "full";

export function App(props: { raw: Raw; said: string[] }) {
  const whole = createMemo(() => digest(props.raw));

  /* Read marks outlive the page, keyed by what was compared as the snapshot adapter named
   * it. The working tree is never the same twice, so it isn't kept. Identities are handed
   * out afresh each run, so marks are stored by file and path instead. */
  const kept = createMemo(() => {
    const pair = compared(props.said);
    return pair && pair[1] !== "current"
      ? `dagger:read:${pair[0]}..${pair[1]}`
      : null;
  });
  const marked = (id: Identity) => {
    const one = whole().definitions.get(id);
    return one ? `${one.file}#${one.path}` : null;
  };
  const stored = (): Set<Identity> => {
    const at = kept();
    if (!at) return new Set();
    try {
      const names = new Set<string>(
        JSON.parse(localStorage.getItem(at) ?? "[]"),
      );
      return new Set(
        [...whole().definitions.keys()].filter((id) => {
          const name = marked(id);
          return name !== null && names.has(name);
        }),
      );
    } catch {
      return new Set();
    }
  };

  /* Starts at 0 so reached-only definitions are opt-in. Capped at what dagger was run
   * with (--ripples), since nothing beyond that exists in the data. */
  const [ripples, setRipples] = createSignal(0);
  const ripple = () =>
    setRipples((was) => (was >= whole().ripples ? 0 : was + 1));

  const review = createMemo(() => narrowed(whole(), ripples()));
  const laid = createMemo(() => layout(review()));
  const names = createMemo(() => namesIn(review()));

  const [read, setRead] = createSignal<Set<Identity>>(stored());
  createEffect(() => {
    const at = kept();
    if (!at) return;
    try {
      localStorage.setItem(
        at,
        JSON.stringify([...read()].flatMap((id) => marked(id) ?? [])),
      );
    } catch {
      /* Storage refused; the marks still last as long as the page. */
    }
  });
  const [panel, setPanel] = createSignal<Opened>(null);
  const toggle = (which: Opened) =>
    setPanel((was) => (was === which ? null : which));
  const [showNext, setShowNext] = createSignal(false);

  const hiding = () =>
    review().warnings.filter((one) => one.impact === "incomplete");
  const weaker = () =>
    review().warnings.filter((one) => one.impact === "degraded");

  /* Wide: a closable, resizable column. Narrow: a drawer, which starts closed and any
   * navigation opens. One intent, clamped to whichever the screen can show. */
  const wide = createMediaQuery(`(min-width: ${WIDE}px)`);
  const [pane, setPane] = createSignal<Pane>("beside");
  const facing = createMemo((): Facing => {
    const want = pane();
    if (wide()) return want === "away" ? "away" : "beside";
    return want === "away" || want === "beside" ? "closed" : want;
  });
  const showing = () => facing() !== "away" && facing() !== "closed";
  const open = () => {
    if (!showing()) setPane(wide() ? "beside" : "half");
  };
  const shut = () => {
    setPane("away");
  };
  const expand = () => setPane(facing() === "full" ? "half" : "full");

  /* Turning ripples down can shrink steps() below the index, so `at` is the clamp. */
  const steps = () => review().steps;
  const [index, setIndex] = createSignal(0);
  const at = createMemo(() =>
    Math.min(index(), Math.max(steps().length - 1, 0)),
  );

  /* `aside` is a definition being viewed that isn't in the reading order (e.g. an
   * unchanged module). Stepping clears it and returns to `at`. */
  const [aside, setAside] = createSignal<Identity | null>(null);
  const here = () => aside() ?? steps()[at()]?.definition ?? null;
  const next = () => steps()[at() + 1]?.definition ?? null;
  const stepping = () => aside() === null;

  const step = (by: number) => {
    setAside(null);
    setIndex(Math.min(Math.max(at() + by, 0), steps().length - 1));
    open();
  };
  const markRead = () => {
    const id = here();
    if (id !== null) setRead((was) => new Set(was).add(id));
  };
  const readAnd = (by: number) => {
    markRead();
    step(by);
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
      setIndex(found);
    } else {
      setAside(id);
    }
    open();
  };

  const [query, setQuery] = createSignal("");
  const matched = createMemo(() =>
    [...review().definitions.values()].filter((one) => matches(one, query())),
  );
  const find = () => {
    const hits = new Set(matched().map((one) => one.id));
    const first = steps().find((step) => hits.has(step.definition));
    if (first) goTo(first.definition);
  };
  let search: HTMLInputElement | undefined;

  useReviewKeys({
    readNext: () => readAnd(1),
    readBack: () => readAnd(-1),
    next: () => step(1),
    back: () => step(-1),
    toggleRead: () => {
      toggleRead();
      open();
    },
    toggleSheet: () => (showing() ? shut() : open()),
    ripples: ripple,
    theme: () => void wear(another(wearing())),
    first: () => setIndex(0),
    last: () => setIndex(steps().length - 1),
    search: () => search?.focus(),
    /* An open panel closes itself on Escape; the sheet only closes when there's none. */
    escape: () => panel() === null && shut(),
  });

  return (
    <>
      <Toolbar
        title={whole().title}
        at={at()}
        total={steps().length}
        hiding={hiding()}
        weaker={weaker()}
        showNext={showNext()}
        rippled={
          whole().ripples > 0 &&
          [...whole().definitions.values()].some((one) => one.reached > 0)
        }
        ripples={ripples()}
        furthest={whole().ripples}
        query={query()}
        matched={matched().length}
        of={review().definitions.size}
        search={(input) => (search = input)}
        onQuery={setQuery}
        onFind={find}
        onWorries={() => toggle("worries")}
        onCost={() => toggle("cost")}
        onShowNext={() => setShowNext((was) => !was)}
        onRipples={ripple}
      />

      <main class={showing() ? `has-sheet ${facing()}` : ""}>
        <div class="stage">
          <Graph
            review={review()}
            laid={laid()}
            here={here()}
            next={showNext() ? next() : null}
            read={read()}
            query={query()}
            onOpen={goTo}
          />
        </div>
        <Reading
          review={review()}
          here={here()}
          step={stepping() ? steps()[at()] : undefined}
          at={at()}
          onStep={step}
          onRead={readAnd}
          onOpen={goTo}
          names={names()}
          read={here() !== null && read().has(here())}
          onToggle={toggleRead}
          sheet={facing()}
          onExpand={expand}
          onClose={shut}
        />
      </main>

      <CostPanel
        open={panel() === "cost"}
        onClose={() => setPanel(null)}
        review={review()}
      />
      <WarningsPanel
        open={panel() === "worries"}
        onClose={() => setPanel(null)}
        hiding={hiding()}
        weaker={weaker()}
      />
    </>
  );
}
