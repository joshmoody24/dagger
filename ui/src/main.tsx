import { render } from "solid-js/web";
import { createResource, Match, Switch } from "solid-js";
import { App } from "./App.tsx";
import { paint } from "./theme.ts";
import "./style.css";

paint();

/* Tauri puts this on the window when the page is running inside one; in a browser it
 * simply isn't there, which is how the page tells where it is. */
declare global {
  interface Window {
    __TAURI__?: { core: { invoke: (command: string, args?: unknown) => Promise<any> } };
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
    const said = await fetch("/review");
    if (!said.ok) throw new Error(await said.text());
    return said.json();
  }

  const said = await tauri.core.invoke("review", {
    repo: await tauri.core.invoke("repo"),
    before: null,
    after: null,
  });
  return JSON.parse(said);
}

function Root() {
  const [review] = createResource(load);

  return (
    <Switch>
      <Match when={review.loading}>
        <p class="waiting">reading the change…</p>
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

render(() => <Root />, document.getElementById("root"));
