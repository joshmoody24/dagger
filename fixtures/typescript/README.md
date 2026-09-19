# A small TypeScript change

Two versions of the same three files, so the TypeScript reading can be checked end to end
without a repository to clone or a commit to find.

`before/` and `after/` are what the two snapshots of a review would hold. The change is the
one from the mock: `Currency` gains a member, `Money` gains a field, and the consequences
travel outward through `addMoney` to `cartTotal` in another file.

What it's here to catch, none of which a unit test does:

- a language server being asked the wrong questions, or the answers being read wrongly
- imports and locals being reported as definitions
- a type gaining a field going unnoticed because hover summarises it away
- the reading order putting a consequence before its cause

Run it with the two directories as the snapshots, which needs no version control at all:

    dagger fixtures/typescript/before fixtures/typescript/after

The expected reading is in `expected.txt`. It is checked by `tests/typescript.rs`, which
skips itself when `tsc` isn't on PATH rather than failing — the fixture is about this
adapter, and not everyone running the tests has a TypeScript compiler.
