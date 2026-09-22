import { Show } from "solid-js";
import {
  ChartColumn,
  Info,
  TriangleAlert,
  Waves,
  Workflow,
} from "lucide-solid";
import type { Warning } from "./dagger.ts";
import "./Toolbar.css";

interface ToolbarProps {
  title: string | null;
  at: number;
  total: number;
  hiding: Warning[];
  weaker: Warning[];
  showNext: boolean;
  /** The ripples control is only offered when the review has something further out. */
  rippled: boolean;
  ripples: number;
  furthest: number;
  query: string;
  /** How many definitions the query matches, out of how many there are. */
  matched: number;
  of: number;
  /** Hands back the input so `/` can focus it from anywhere. */
  search: (input: HTMLInputElement) => void;
  onQuery: (query: string) => void;
  onFind: () => void;
  onWarnings: () => void;
  onCost: () => void;
  onShowNext: () => void;
  onRipples: () => void;
}

export function Toolbar(props: ToolbarProps) {
  const said = () =>
    props.hiding.length
      ? `${props.hiding.length} this review might not be showing`
      : `${props.weaker.length} worked out a weaker way`;

  return (
    <header>
      <Show when={props.title}>
        <h1 class="title">{props.title}</h1>
      </Show>
      <div class="toolbar">
        <span class="prog">
          {props.total === 0
            ? "nothing to read"
            : `${props.at + 1}/${props.total}`}
        </span>
        <label class="find">
          <input
            ref={props.search}
            type="search"
            placeholder="filter"
            aria-label="Filter definitions"
            value={props.query}
            onInput={(event) => props.onQuery(event.currentTarget.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter") props.onFind();
              else if (event.key === "Escape") {
                props.onQuery("");
                event.currentTarget.blur();
              } else return;
              event.preventDefault();
            }}
          />
          <Show when={props.query}>
            <span class="found">
              {props.matched} of {props.of}
            </span>
          </Show>
        </label>
        {/* Warning icon only for "incomplete"; "degraded" is info so alarms stay meaningful. */}
        <Show when={props.hiding.length + props.weaker.length}>
          <button
            class={`tool${props.hiding.length ? " bad" : ""}`}
            onClick={() => props.onWarnings()}
            title={said()}
            aria-label={said()}
          >
            <Show when={props.hiding.length} fallback={<Info size={17} />}>
              <TriangleAlert size={17} />
            </Show>
          </button>
        </Show>

        <button
          class="tool"
          onClick={() => props.onCost()}
          title="Cognitive load metrics"
          aria-label="Cognitive load metrics"
        >
          <ChartColumn size={17} />
        </button>

        <button
          class={`tool${props.showNext ? " on" : ""}`}
          onClick={() => props.onShowNext()}
          title="Show where the reading goes next"
          aria-label="Show where the reading goes next"
          aria-pressed={props.showNext}
        >
          <Workflow size={17} />
        </button>

        <Show when={props.rippled}>
          <button
            class={`tool steps${props.ripples ? " on" : ""}`}
            onClick={() => props.onRipples()}
            title={`Ripples: showing what the change reached ${props.ripples} of ${props.furthest} steps out (r)`}
            aria-label={`Ripples: ${props.ripples} of ${props.furthest} steps out`}
          >
            <Waves size={17} />
            <b>{props.ripples}</b>
          </button>
        </Show>
      </div>
    </header>
  );
}
