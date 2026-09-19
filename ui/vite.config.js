import { spawn } from "node:child_process";
import { defineConfig } from "vite";
import solid from "vite-plugin-solid";

/* Where the page gets a review when it's open in a browser rather than in the window.
 *
 * The window asks dagger through Tauri. A browser has no way to run anything, so it asks
 * here and this runs the same command — `dagger --json`, no revisions, which is the working
 * changes. Reading a file instead is what made the page quietly show yesterday's answer
 * every time anything changed.
 *
 * The fixture the tests run against is a different thing and stays pinned: a test wants the
 * same input every time, and a page wants the current one.
 */
function live() {
  return {
    name: "dagger-review",
    configureServer(server) {
      server.middlewares.use("/review", (request, response) => {
        /* Which pair to read. Named in the address when the page asks for a particular
         * one, and otherwise whatever this server was started on — so a browser opened
         * beside a window started on `HEAD~1 HEAD` is looking at the same change, rather
         * than quietly at a different one. Neither, and dagger decides, which means the
         * working changes. */
        const asked = new URL(request.url, "http://dagger").searchParams;
        const before = asked.get("before") || process.env.DAGGER_BEFORE;
        const after = asked.get("after") || process.env.DAGGER_AFTER;
        const reading = before && after ? [before, after] : [];

        const dagger = spawn("target/debug/dagger", ["--json", ...reading], { cwd: ".." });

        let said = "";
        let wrong = "";
        dagger.stdout.on("data", (chunk) => (said += chunk));
        // Dagger says what it's up to on the way, and it takes long enough to be worth
        // seeing rather than watching a blank page.
        dagger.stderr.on("data", (chunk) => {
          wrong += chunk;
          process.stderr.write(chunk);
        });

        dagger.on("error", (error) => {
          response.statusCode = 500;
          response.end(`couldn't run dagger: ${error.message}`);
        });
        dagger.on("close", (code) => {
          if (code !== 0) {
            response.statusCode = 500;
            response.end(wrong.trim() || `dagger gave up with ${code}`);
            return;
          }
          response.setHeader("content-type", "application/json");
          response.end(said);
        });
      });
    },
  };
}

export default defineConfig({
  plugins: [solid(), live()],
  // Tauri looks for the page here while developing, and for the build in dist.
  server: { port: 1420, strictPort: true },
  build: { target: "esnext" },
});
