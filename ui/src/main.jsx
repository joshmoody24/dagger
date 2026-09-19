import { render } from "solid-js/web";
import { createResource, Match, Switch } from "solid-js";
import { App } from "./App.jsx";
import { paint } from "./theme.js";
import "./style.css";

paint();

/* Where the review comes from.
 *
 * In a window, dagger is asked for one. In a browser — which is how this gets worked on —
 * a saved one is read from disk, so the page can be looked at without a repository or a
 * language server anywhere in sight.
 */
async function load() {
  const tauri = window.__TAURI__;
  if (!tauri) {
    return (await fetch("/sample.json")).json();
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
