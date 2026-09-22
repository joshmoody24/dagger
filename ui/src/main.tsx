import { render } from "solid-js/web";
import { createResource, createSignal, Match, Switch } from "solid-js";
import { App } from "./App.tsx";
import { Waiting } from "./Waiting.tsx";
import { themes, wear } from "./theme.ts";
import "./style.css";

await wear(themes[0]);

/* Tauri puts this on the window when the page is running inside one; in a browser it
 * simply isn't there, which is how the page tells where it is. */
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

/* Where the review comes from.
 *
 * Either way it's dagger that's asked, and asked for the same thing: the working changes.
 * In a window that goes through Tauri; in a browser it goes to the dev server, which runs
 * the same command. Nothing here reads a saved review — one that's written down is out of
 * date as soon as anything changes, and the page can't tell.
 */
async function load() {
  const tauri = window.__TAURI__;
  if (!tauri) {
    /* Whatever the address asks for is passed straight on, so a link to one change is a
     * link somebody else can open. */
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

/* A line of JSON at a time: whatever dagger said as it said it, and the review last. */
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

/* What dagger has said so far, which is how far along it is. */
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
