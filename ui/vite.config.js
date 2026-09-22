import { spawn } from "node:child_process";
import { defineConfig } from "vite";
import solid from "vite-plugin-solid";

/* Where the page gets a review while it's being developed here rather than served by
 * `dagger open`.
 *
 * The window asks dagger through Tauri, and `dagger open` runs dagger itself. A browser
 * has no way to run anything, so it asks here and this runs the same command — `dagger
 * --json`, no revisions, which is the working changes. Reading a file instead is what made
 * the page quietly show yesterday's answer every time anything changed. `dagger open`
 * answers /review the same way, line for line, so the page can't tell them apart.
 *
 * The fixture the tests run against is a different thing and stays pinned: a test wants the
 * same input every time, and a page wants the current one.
 */
function live() {
  return {
    name: "dagger-review",
    configureServer(server) {
      server.middlewares.use("/review", (request, response) => {
        /* What to read, passed to dagger as typed — the words are its snapshot adapter's,
         * not this server's. Named in the address when the page asks for something
         * particular, and otherwise whatever this server was started on, so a browser
         * opened beside a window is looking at the same change rather than quietly at a
         * different one. Neither, and dagger decides, which means the working changes. */
        const asked = new URL(request.url, "http://dagger").searchParams;
        const reading = asked.getAll("read").length
          ? asked.getAll("read")
          : (process.env.DAGGER_READ || "").split("\n").filter(Boolean);

        const dagger = spawn("target/debug/dagger", ["--json", ...reading], {
          cwd: "..",
        });

        /* Sent as it happens rather than all at once at the end. Dagger says what it's
         * doing on the way — which snapshot, which extractor, how far through — and a
         * reader waiting a minute deserves to see that rather than a still page. One line
         * of JSON per thing said, and the review itself as the last of them. */
        response.setHeader("content-type", "application/x-ndjson");
        response.setHeader("cache-control", "no-store");

        let said = "";
        let wrong = "";
        let left = "";

        /* The review is one line, handed over as soon as it ends: dagger still has its
         * snapshots to remove afterwards. */
        dagger.stdout.on("data", (chunk) => {
          said += chunk;
          const end = said.indexOf("\n");
          if (end < 0 || response.writableEnded) return;
          response.write(
            `${JSON.stringify({ review: JSON.parse(said.slice(0, end)) })}\n`,
          );
          response.end();
        });
        dagger.stderr.on("data", (chunk) => {
          wrong += chunk;
          process.stderr.write(chunk);

          left += chunk;
          const lines = left.split("\n");
          left = lines.pop() ?? "";
          for (const note of lines)
            response.write(`${JSON.stringify({ note })}\n`);
        });

        dagger.on("error", (error) => {
          response.write(
            `${JSON.stringify({ wrong: `couldn't run dagger: ${error.message}` })}\n`,
          );
          response.end();
        });
        dagger.on("close", (code) => {
          if (response.writableEnded) return;
          const why = wrong.trim() || `dagger gave up with ${code}`;
          response.write(`${JSON.stringify({ wrong: why })}\n`);
          response.end();
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
