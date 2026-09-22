import { render } from "solid-js/web";
import { createResource, createSignal, Match, Switch } from "solid-js";
import { App } from "./App.tsx";
import { Waiting } from "./Waiting.tsx";
import { themes, wear } from "./theme.ts";
import "./style.css";

await wear(themes[0]);

/* Present only when running inside Tauri; its absence means we're in a browser. */
declare global {
  interface Window {
    __TAURI__?: {
      core: { invoke: (command: string, args?: unknown) => Promise<any> };
      event: {
        listen: (
          name: string,
          heard: (sent: { payload: string }) => void,
        ) => Promise<() => void>;
      };
    };
  }
}

/* Always runs dagger fresh (via Tauri or the dev server) rather than reading a saved
 * review, which would silently go stale. */
async function load() {
  const tauri = window.__TAURI__;
  if (!tauri) {
    /* Query string is forwarded so a URL for one change is shareable. */
    const said = await fetch(`/review${window.location.search}`);
    if (!said.ok) throw new Error(await said.text());
    return await streamed(said);
  }

  const off = await tauri.event.listen("dagger://said", (sent) =>
    setSaid((was) => [...was, sent.payload]),
  );
  try {
    const said = await tauri.core.invoke(
      "review",
      await tauri.core.invoke("opened"),
    );
    return JSON.parse(said);
  } finally {
    off();
  }
}

/* Newline-delimited JSON: progress notes, then the review last. */
async function streamed(said: Response) {
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

const [said, setSaid] = createSignal<string[]>([]);

function Root() {
  const [review] = createResource(load);

  return (
    <Switch>
      <Match when={review.loading}>
        <Waiting said={said()} />
      </Match>
      <Match when={review.error}>
        <p class="waiting">{String(review.error)}</p>
      </Match>
      <Match when={review()}>
        <App raw={review()} />
      </Match>
    </Switch>
  );
}

render(() => <Root />, document.getElementById("root")!);
