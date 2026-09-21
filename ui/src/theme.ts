import { createSignal } from "solid-js";

/* The colours, taken from an editor theme rather than picked by hand.
 *
 * Every colour the page uses is named for what it's for — the ground, a rule, the thing that
 * broke — and this is the one place those names are given values. Editor themes already
 * answer the same questions (what's a background, what's a comment, what's a deletion), and
 * hundreds of them exist, so wearing a different one is a question of which, not of editing
 * anything.
 *
 * Only what this page needs is read; the rest of a theme describes syntax we don't colour.
 */

/* The ones worth looking at, brought in one at a time — each is a separate import so only
 * the one being worn is ever fetched.
 *
 * All of them sit on something close to black. A theme built on a lighter grey has to keep
 * its text away from white to look considered, and what's left is a narrow band that tires
 * the eye: measured across every dark theme available, the near-black ones run 13 to 18
 * times the contrast of their own background, and the grey ones around 8. */
const WEARING: Record<string, () => Promise<{ default: Theme }>> = {
  "github-dark-high-contrast": () => import("@shikijs/themes/github-dark-high-contrast"),
  vesper: () => import("@shikijs/themes/vesper"),
  "github-dark-default": () => import("@shikijs/themes/github-dark-default"),
  "vitesse-black": () => import("@shikijs/themes/vitesse-black"),
  houston: () => import("@shikijs/themes/houston"),
  "night-owl": () => import("@shikijs/themes/night-owl"),
  "vitesse-dark": () => import("@shikijs/themes/vitesse-dark"),
  "dark-plus": () => import("@shikijs/themes/dark-plus"),
};

/* Shiki's own shape, loosely: a theme may leave out anything, which is why every role
 * below says what it will settle for. */
interface Theme {
  type?: string;
  colors?: Record<string, string>;
  tokenColors?: { scope?: string | string[]; settings?: { foreground?: string } }[];
}

export const themes = Object.keys(WEARING);

/* Which one is on. Read it and a change to it redraws, which is how the graph — painted
 * rather than styled — hears about a colour it can't be told by a stylesheet. */
const [worn, setWorn] = createSignal(themes[0]);
export const wearing = worn;

/* The theme itself, kept so whatever colours code can use the same one. */
const [dressed, setDressed] = createSignal<Theme | null>(null);
export const dressing = dressed;

export async function wear(name: string, root = document.documentElement) {
  const { default: theme } = await WEARING[name]();
  paint(theme, root);
  setDressed(theme as Theme);
  setWorn(name);
}

/** The next one along, so a key can walk the list. */
export const next = (name: string) => themes[(themes.indexOf(name) + 1) % themes.length];

/* Themes don't all set every key, so each colour says what it would rather have first, and
 * what it will settle for. */
const ROLES: Record<string, string[]> = {
  paper: ["sideBar.background", "editorGroupHeader.tabsBackground", "editor.background"],
  surface: ["editor.background", "sideBar.background"],
  raised: ["list.activeSelectionBackground", "editor.lineHighlightBackground", "editor.background"],
  ink: ["editor.foreground", "foreground"],
  muted: ["editor.foreground", "peekViewResult.lineForeground", "foreground"],
  /* Decoration only — an edge, a guide. Never text: this is the colour an editor uses for
   * line numbers, which it means you not to notice. */
  faint: ["editorLineNumber.foreground", "editorIndentGuide.activeBackground1", "editorIndentGuide.background1"],
  rule: ["editorBracketMatch.border", "panelTitle.inactiveForeground", "diffEditor.diagonalFill", "editorGroup.border"],
  lean: ["terminal.ansiBlue", "textLink.foreground", "editorLink.activeForeground"],
  /* Kept apart from lean on purpose: one says where you are, the other where you're going,
   * and they're side by side on screen. */
  path: ["terminal.ansiMagenta", "textLink.activeForeground", "terminal.ansiCyan"],
  add: ["terminal.ansiGreen", "gitDecoration.addedResourceForeground", "editorGutter.addedBackground"],
  del: ["terminal.ansiRed", "gitDecoration.deletedResourceForeground", "editorGutter.deletedBackground"],
  chg: ["terminal.ansiYellow", "gitDecoration.modifiedResourceForeground", "editorGutter.modifiedBackground"],
  addbg: ["diffEditor.insertedLineBackground", "diffEditor.insertedTextBackground"],
  delbg: ["diffEditor.removedLineBackground", "diffEditor.removedTextBackground"],
};

/* When a theme says nothing at all about a role, something still has to be there: a colour
 * left unset isn't a plainer page, it's whichever colour the last theme put in its place. */
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

  /* The colour of an ordinary name in code — brighter than editor.foreground in every
   * theme that distinguishes them, and the one a reader's eye is calibrated to. */
  const plain = scoped(theme, "variable");
  if (plain) root.style.setProperty("--ink", plain);

  /* Comments recede in an editor because bright syntax surrounds them. Here they sit in a
   * small block on their own, so the editor's dimmest readable grey serves better than its
   * comment colour, which is dimmer still. */
  root.style.setProperty(
    "--quiet",
    theme.colors?.["editorLineNumber.activeForeground"] ||
      scoped(theme, "comment") ||
      INSTEAD.muted,
  );
  root.style.colorScheme = theme.type === "light" ? "light" : "dark";
}

/* What the theme paints one kind of token. A scope is listed against several settings and
 * only some of them say a colour, so the first one that does wins. */
function scoped(theme: Theme, scope: string) {
  for (const rule of theme.tokenColors || []) {
    const scopes = ([] as string[]).concat(rule.scope || []);
    if (scopes.includes(scope) && rule.settings && rule.settings.foreground) {
      return rule.settings.foreground;
    }
  }
  return null;
}
