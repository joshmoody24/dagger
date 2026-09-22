import { render } from "solid-js/web";
import {
  createResource,
  createSignal,
  Match,
  type Setter,
  Switch,
} from "solid-js";
import { App } from "./App.tsx";
import { Waiting } from "./Waiting.tsx";
import { themes, wear } from "./theme.ts";
import "./base.css";

await wear(themes[0]);

/* Always runs dagger fresh, through whichever server is behind the page, rather than
 * reading a saved review, which would silently go stale. */
async function load(setSaid: Setter<string[]>) {
  /* Query string is forwarded so a URL for one change is shareable. */
  const said = await fetch(`/review${window.location.search}`);
  if (!said.ok) throw new Error(await said.text());
  return await streamed(said, setSaid);
}

/* Newline-delimited JSON: progress notes, then the review last. */
async function streamed(said: Response, setSaid: Setter<string[]>) {
  const reader = said.body!.getReader();
  const words = new TextDecoder();
  let left = "";

  for (;;) {
    const { done, value } = await reader.read();
    left += value ? words.decode(value, { stream: true }) : "";

    const lines = left.split("\n");
    left = done ? "" : (lines.pop() ?? "");

    for (const line of lines) {
      if (!line.trim()) continue;
      const sent = JSON.parse(line);
      if (sent.review) return sent.review;
      if (sent.wrong) throw new Error(sent.wrong);
      setSaid((was) => [...was, sent.note]);
    }
    if (done) throw new Error("dagger stopped without saying anything");
  }
}

function Root() {
  const [said, setSaid] = createSignal<string[]>([]);
  const [review] = createResource(() => load(setSaid));

  return (
    <Switch>
      <Match when={review.loading}>
        <Waiting said={said()} />
      </Match>
      <Match when={review.error}>
        <p class="waiting">{String(review.error)}</p>
      </Match>
      <Match when={review()}>
        <App raw={review()} said={said()} />
      </Match>
    </Switch>
  );
}

render(() => <Root />, document.getElementById("root")!);
