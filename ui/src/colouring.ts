import { createHighlighterCore, type HighlighterCore } from "shiki/core";
import { createJavaScriptRegexEngine } from "shiki/engine/javascript";
import { createSignal } from "solid-js";

/* Colouring code the way an editor does, with the grammar an editor uses.
 *
 * What was here before split a line into comments, strings, names and everything else,
 * which is four colours where an editor has twenty: a keyword, a number and a bracket all
 * came out the same, so most of the page was one colour. Shiki reads the same TextMate
 * grammars, which is what makes its answer look like the editor the reader came from —
 * and it takes the theme already being worn, so the two can't disagree.
 *
 * A whole block at once, not a line at a time. A grammar carries state from one line to the
 * next — inside a block comment, inside a template string — so a line handed over on its own
 * is read as if the file started there, and the middle of a comment comes back as code.
 */

/* Only the languages this can be pointed at so far. A grammar is a few hundred kilobytes,
 * so they're fetched when a file needs one and not before. */
const GRAMMARS: Record<string, () => Promise<unknown>> = {
  typescript: () => import("@shikijs/langs/typescript"),
  tsx: () => import("@shikijs/langs/tsx"),
  javascript: () => import("@shikijs/langs/javascript"),
  jsx: () => import("@shikijs/langs/jsx"),
  rust: () => import("@shikijs/langs/rust"),
  python: () => import("@shikijs/langs/python"),
  go: () => import("@shikijs/langs/go"),
  c: () => import("@shikijs/langs/c"),
  cpp: () => import("@shikijs/langs/cpp"),
  json: () => import("@shikijs/langs/json"),
  css: () => import("@shikijs/langs/css"),
};

const SPEAKS: Record<string, string> = {
  ts: "typescript", mts: "typescript", cts: "typescript",
  tsx: "tsx", js: "javascript", mjs: "javascript", cjs: "javascript", jsx: "jsx",
  rs: "rust", py: "python", go: "go",
  c: "c", h: "c", cc: "cpp", cpp: "cpp", hpp: "cpp",
  json: "json", css: "css",
};

/** Which grammar a file is written in, as far as this knows any. */
export const speaks = (file: string) => SPEAKS[file.split(".").pop() ?? ""] ?? null;

export interface Painted {
  text: string;
  colour?: string | undefined;
}

let ready: HighlighterCore | undefined;
const loading = new Set<string>();

/* Bumped whenever a grammar or theme finishes loading, so anything showing code can ask
 * again now there's an answer. */
const [settled, setSettled] = createSignal(0);
export const colouring = settled;

/** Gets ready to colour this language in this theme, if it isn't already. */
export async function readied(language: string | null, theme: any, name: string) {
  if (!language || !GRAMMARS[language]) return;

  const asked = `${name}:${language}`;
  if (loading.has(asked)) return;
  loading.add(asked);

  try {
    if (!ready) {
      ready = await createHighlighterCore({
        themes: [],
        langs: [],
        engine: createJavaScriptRegexEngine(),
      });
    }
    if (!ready.getLoadedThemes().includes(name)) {
      await ready.loadTheme({ ...theme, name });
    }
    if (!ready.getLoadedLanguages().includes(language)) {
      await ready.loadLanguage((await GRAMMARS[language]()) as any);
    }
    setSettled((was) => was + 1);
  } catch {
    /* A grammar that won't load leaves the code plain, which is worse to look at and
     * right in every other way. */
    loading.delete(asked);
  }
}

/** Lines of code, each split into the pieces a theme paints differently. */
export function painted(lines: string[], language: string | null, theme: string): Painted[][] {
  const plain = () => lines.map((line) => [{ text: line }]);
  if (!ready || !language) return plain();
  if (!ready.getLoadedLanguages().includes(language)) return plain();
  if (!ready.getLoadedThemes().includes(theme)) return plain();

  const { tokens } = ready.codeToTokens(lines.join("\n"), { lang: language, theme });
  return lines.map((line, at) =>
    (tokens[at] ?? [{ content: line }]).map((token) => ({
      text: token.content,
      colour: (token as { color?: string }).color,
    })),
  );
}
