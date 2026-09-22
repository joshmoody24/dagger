# Working on dagger

## Vocabulary

- One word per concept. The words are defined in docs/model.md; use them in code,
  comments, UI text, CLI output, and docs. No synonyms.
- The product name is `dagger`, never capitalized.
- Snapshot, not repository or revision or side. Extractor adapter and snapshot adapter,
  both kinds of adapter. Group, not box. Definition, not node. Contract, not type.
  Depends on, not leans on or uses. Read, not viewed. Reached, not affected.

## Code

- Net-negative or net-neutral is the goal. If fixing a bug adds code, ask whether the
  design is wrong before adding it.
- No band-aids: nothing in the core or the git adapter may know about a language, a
  package manager, or a directory name like `node_modules`.
- The core is generic. What a snapshot is, what a revision is, and how files are found
  belong to adapters. The page draws the review and decides nothing.
- Prefer functional, immutable, declarative code. Iterator chains over loops that push.
  Derived state over stored state. Invalid states unrepresentable over assertions.
- Comments say why, never what. Three lines is the limit. Plain language. No narratives
  about past bugs; a test locks that in instead.
- Sentences with semicolons are two sentences. Avoid parentheses and pronouns in prose.

## Process

- Measure before claiming. If a change should be faster or shouldn't change output,
  prove it: time it, or diff the JSON against a baseline.
- Look at the page. Unit tests passed while the gutter was broken; drive headless Chrome
  or run `dagger gui` and check.
- Make edits with file tools, not shell, so diffs are reviewable. Small diffs for prose:
  one section at a time.
- One commit per concept. Commit messages are one short line.
- `./check` must pass before every commit. It runs everything CI runs.
- Keyboard shortcuts live on the right half of the keyboard.
