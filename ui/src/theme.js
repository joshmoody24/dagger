/* The colours, taken from an editor theme rather than picked by hand.
 *
 * Every colour the page uses is named for what it's for — the ground, a rule, the thing that
 * broke — and this is the one place those names are given values. Editor themes already
 * answer the same questions (what's a background, what's a comment, what's a deletion), and
 * hundreds of them exist, so swapping the whole look means changing the import below.
 *
 * Only what this page needs is read; the rest of a theme describes syntax we don't colour.
 */
import theme from "@shikijs/themes/tokyo-night";

/* Themes don't all set every key, so each colour says what it would rather have first. */
const ROLES = {
  paper: ["sideBar.background", "editorGroupHeader.tabsBackground", "editor.background"],
  surface: ["editor.background"],
  raised: ["list.activeSelectionBackground", "editor.lineHighlightBackground", "editor.background"],
  /* Not editor.foreground: that's the colour of punctuation and whatever the theme has no
   * opinion about, and it sits well below the colour real code is written in. The one used
   * for plain identifiers is what an editor actually looks like to read. */
  ink: ["editor.foreground", "foreground"],
  muted: ["editor.foreground", "peekViewResult.lineForeground"],
  /* Decoration only — an edge, a guide. Never text: this is the colour an editor uses for
   * line numbers, which it means you not to notice. */
  faint: ["editorLineNumber.foreground", "editorIndentGuide.activeBackground1"],
  rule: ["editorBracketMatch.border", "panelTitle.inactiveForeground", "diffEditor.diagonalFill"],
  lean: ["terminal.ansiBlue", "textLink.foreground"],
  add: ["terminal.ansiGreen", "gitDecoration.addedResourceForeground"],
  del: ["terminal.ansiRed", "gitDecoration.deletedResourceForeground"],
  chg: ["terminal.ansiYellow", "gitDecoration.modifiedResourceForeground"],
  addbg: ["diffEditor.insertedLineBackground", "diffEditor.insertedTextBackground"],
  delbg: ["diffEditor.removedLineBackground", "diffEditor.removedTextBackground"],
};

export function paint(root = document.documentElement) {
  for (const [name, keys] of Object.entries(ROLES)) {
    const found = keys.map((key) => theme.colors[key]).find(Boolean);
    if (found) root.style.setProperty(`--${name}`, found);
  }

  /* The colour of an ordinary name in code — brighter than editor.foreground in every
   * theme that distinguishes them, and the one a reader's eye is calibrated to. */
  const plain = scoped("variable");
  if (plain) root.style.setProperty("--ink", plain);

  /* Comments recede in an editor because bright syntax surrounds them. Here they sit in a
   * small block on their own, so the editor's dimmest readable grey serves better than its
   * comment colour, which is dimmer still. */
  root.style.setProperty(
    "--quiet",
    theme.colors["editorLineNumber.activeForeground"] || scoped("comment"),
  );
  root.style.colorScheme = theme.type === "light" ? "light" : "dark";
}

/* What the theme paints one kind of token. A scope is listed against several settings and
 * only some of them say a colour, so the first one that does wins. */
function scoped(scope) {
  for (const rule of theme.tokenColors || []) {
    const scopes = [].concat(rule.scope || []);
    if (scopes.includes(scope) && rule.settings && rule.settings.foreground) {
      return rule.settings.foreground;
    }
  }
  return null;
}
