import { createSignal } from "solid-js";

/* Colours come from an editor theme rather than being picked by hand: themes already answer
 * the same questions (background, comment, deletion) and there are hundreds to choose from.
 * Only the roles this page uses are read. */

/* Separate dynamic imports so only the theme being worn is fetched. All near-black: grey
 * dark themes have far less contrast against their own background (about 8x vs 13-18x)
 * and tire the eye. */
const WEARING: Record<string, () => Promise<{ default: Theme }>> = {
  "github-dark-high-contrast": () =>
    import("@shikijs/themes/github-dark-high-contrast"),
  vesper: () => import("@shikijs/themes/vesper"),
  "github-dark-default": () => import("@shikijs/themes/github-dark-default"),
  "vitesse-black": () => import("@shikijs/themes/vitesse-black"),
  houston: () => import("@shikijs/themes/houston"),
  "night-owl": () => import("@shikijs/themes/night-owl"),
  "vitesse-dark": () => import("@shikijs/themes/vitesse-dark"),
  "dark-plus": () => import("@shikijs/themes/dark-plus"),
};

/* Shiki's shape, loosely. A theme may leave out anything. */
export interface Theme {
  type?: string;
  colors?: Record<string, string>;
  tokenColors?: {
    scope?: string | string[];
    settings?: { foreground?: string };
  }[];
}

export const themes = Object.keys(WEARING);

/* A signal so the page can show which theme it's wearing. */
const [worn, setWorn] = createSignal(themes[0]);
export const wearing = worn;

/* Kept so the code highlighter can use the same theme. */
const [dressed, setDressed] = createSignal<Theme | null>(null);
export const dressing = dressed;

export async function wear(name: string, root = document.documentElement) {
  const { default: theme } = await WEARING[name]();
  paint(theme, root);
  setDressed(theme);
  setWorn(name);
}

/** The next one along, so a key can walk the list. */
export const next = (name: string) =>
  themes[(themes.indexOf(name) + 1) % themes.length];

/* Themes don't set every key, so each role lists keys in order of preference. */
const ROLES: Record<string, string[]> = {
  paper: [
    "sideBar.background",
    "editorGroupHeader.tabsBackground",
    "editor.background",
  ],
  surface: ["editor.background", "sideBar.background"],
  raised: [
    "list.activeSelectionBackground",
    "editor.lineHighlightBackground",
    "editor.background",
  ],
  ink: ["editor.foreground", "foreground"],
  muted: ["editor.foreground", "peekViewResult.lineForeground", "foreground"],
  /* Decoration only, never text: this is the line-number colour, meant not to be noticed. */
  faint: [
    "editorLineNumber.foreground",
    "editorIndentGuide.activeBackground1",
    "editorIndentGuide.background1",
  ],
  rule: [
    "editorBracketMatch.border",
    "panelTitle.inactiveForeground",
    "diffEditor.diagonalFill",
    "editorGroup.border",
  ],
  lean: [
    "terminal.ansiBlue",
    "textLink.foreground",
    "editorLink.activeForeground",
  ],
  /* Distinct from lean on purpose: the two sit side by side on screen. */
  path: [
    "terminal.ansiMagenta",
    "textLink.activeForeground",
    "terminal.ansiCyan",
  ],
  add: [
    "terminal.ansiGreen",
    "gitDecoration.addedResourceForeground",
    "editorGutter.addedBackground",
  ],
  del: [
    "terminal.ansiRed",
    "gitDecoration.deletedResourceForeground",
    "editorGutter.deletedBackground",
  ],
  chg: [
    "terminal.ansiYellow",
    "gitDecoration.modifiedResourceForeground",
    "editorGutter.modifiedBackground",
  ],
  addbg: [
    "diffEditor.insertedLineBackground",
    "diffEditor.insertedTextBackground",
  ],
  delbg: [
    "diffEditor.removedLineBackground",
    "diffEditor.removedTextBackground",
  ],
};

/* A role left unset would keep whatever colour the previous theme put there. */
const INSTEAD: Record<string, string> = {
  paper: "#111111",
  surface: "#161616",
  raised: "#202020",
  ink: "#e6e6e6",
  muted: "#a0a0a0",
  faint: "#3a3a3a",
  rule: "#4a4a4a",
  lean: "#6aa9ff",
  path: "#c58af9",
  add: "#5fc58f",
  del: "#ef7c72",
  chg: "#e3a54b",
  addbg: "#1e3a2a",
  delbg: "#3a1f22",
};

function paint(theme: Theme, root: HTMLElement) {
  for (const [name, keys] of Object.entries(ROLES)) {
    const found = keys.map((key) => theme.colors?.[key]).find(Boolean);
    root.style.setProperty(`--${name}`, found || INSTEAD[name]);
  }

  /* The plain variable colour is brighter than editor.foreground in most themes, and it's
   * what readers are used to. */
  const plain = scoped(theme, "variable");
  if (plain) root.style.setProperty("--ink", plain);

  /* The theme's comment colour is too dim once comments aren't surrounded by bright
   * syntax, so use the editor's dimmest readable grey instead. */
  root.style.setProperty(
    "--quiet",
    theme.colors?.["editorLineNumber.activeForeground"] ||
      scoped(theme, "comment") ||
      INSTEAD.muted,
  );
  root.style.colorScheme = theme.type === "light" ? "light" : "dark";
}

/* First rule for the scope that actually sets a foreground. */
function scoped(theme: Theme, scope: string) {
  const rule = (theme.tokenColors || []).find(
    (rule) =>
      ([] as string[]).concat(rule.scope || []).includes(scope) &&
      rule.settings?.foreground,
  );
  return rule?.settings?.foreground ?? null;
}
