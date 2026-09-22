import {
  createHighlighterCore,
  type HighlighterCore,
  type ThemeRegistrationAny,
} from "shiki/core";
import { createJavaScriptRegexEngine } from "shiki/engine/javascript";
import { createSignal } from "solid-js";
import type { Theme } from "./theme.ts";

/* Shiki, using the theme already being worn, so code looks like the reader's editor. Whole
 * blocks are highlighted at once because grammars carry state between lines (block
 * comments, template strings). */

/* Grammars are a few hundred kilobytes each, so they're fetched on demand. */
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
  ts: "typescript",
  mts: "typescript",
  cts: "typescript",
  tsx: "tsx",
  js: "javascript",
  mjs: "javascript",
  cjs: "javascript",
  jsx: "jsx",
  rs: "rust",
  py: "python",
  go: "go",
  c: "c",
  h: "c",
  cc: "cpp",
  cpp: "cpp",
  hpp: "cpp",
  json: "json",
  css: "css",
};

/** Which grammar a file is written in, as far as this knows any. */
export const speaks = (file: string) =>
  SPEAKS[file.split(".").pop() ?? ""] ?? null;

export interface Painted {
  text: string;
  colour?: string | undefined;
}

let ready: HighlighterCore | undefined;
const loading = new Set<string>();

/* Bumped when a grammar or theme finishes loading so code re-renders. */
const [settled, setSettled] = createSignal(0);
export const colouring = settled;

/** Gets ready to colour this language in this theme, if it isn't already. */
export async function readied(
  language: string | null,
  theme: Theme | null,
  name: string,
) {
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
      await ready.loadTheme({ ...theme, name } as ThemeRegistrationAny);
    }
    if (!ready.getLoadedLanguages().includes(language)) {
      await ready.loadLanguage((await GRAMMARS[language]()) as any);
    }
    setSettled((was) => was + 1);
  } catch {
    /* Leave the code plain and allow a retry. */
    loading.delete(asked);
  }
}

/** Lines of code, each split into the pieces a theme paints differently. */
export function painted(
  lines: string[],
  language: string | null,
  theme: string,
): Painted[][] {
  const plain = () => lines.map((line) => [{ text: line }]);
  if (!ready || !language) return plain();
  if (!ready.getLoadedLanguages().includes(language)) return plain();
  if (!ready.getLoadedThemes().includes(theme)) return plain();

  const { tokens } = ready.codeToTokens(lines.join("\n"), {
    lang: language,
    theme,
  });
  return lines.map((line, at) =>
    (tokens[at] ?? [{ content: line }]).map((token) => ({
      text: token.content,
      colour: (token as { color?: string }).color,
    })),
  );
}
