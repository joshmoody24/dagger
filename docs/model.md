# Dagger's data model

This is the vocabulary dagger uses and how the words relate. The Rust types in
`crates/core/src` are the source of truth.

## A review

The whole data model of dagger is the **review.**

A **review** is a reading of the difference between two code
snapshots, optimized for understanding. `dagger` generates a review using the following process:

1. **Before** and **after** **snapshots** of the code are laid out on disk by the
   **snapshot adapter**, such as git.
2. In each snapshot, the **extractor adapters** read their assigned files. For every named
   thing in a file, they report an **occurrence**. An occurrence has a **locator**, which is
   the file and a path of names, and a list of **pieces**. A piece is a stretch of text that
   belongs to one **part**. The parts are the **contract**, **body**, and **docs**.
   These parts are separated because changes to each are treated differently later on.
   Beside the occurrences, the extractor adapters
   report **mentions**. A mention is a place where one occurrence names another.
3. The occurrences of the two snapshots are matched. An occurrence in the before snapshot
   and one in the after snapshot that are the same named thing become one **definition**
   with one **identity**. A definition's **change** is what differs between its two
   occurrences.
4. Mentions become **references** between definitions. References are followed outward
   from what changed, as many steps as the **ripples** setting allows, to find what a change
   **reached**. The references among the definitions shown become **edges**. An edge says
   what depends on what.
5. The **reading** is a list of **steps**. Each step is one
   definition. The reading order is optimized for understanding. In general, the order starts at the core changes
   and follows the changes from there, while minimizing the number of concepts you have to hold in your head.
6. The definitions are shaped into **groups**, the boxes on the page.
7. The page in the browser, or the terminal, renders the review.

## The core model

The `Review` type holds everything above. Every other type hangs off its definitions.

```mermaid
flowchart LR
    Review["<b>Review</b><br>title<br>ripples"]
    Definition["<b>Definition</b><br>id<br>role<br>change<br>parent"]
    Occurrence["<b>Occurrence</b><br>file<br>kind"]
    Locator["<b>Locator</b><br>scope<br>name"]
    Piece["<b>Piece</b><br>part<br>text<br>line"]
    Edge["<b>Edge</b><br>from<br>to"]
    Step["<b>Step</b><br>definition<br>on_faith"]
    Group["<b>Group</b><br>name<br>tier"]

    Review -- "definitions 1..*" --> Definition
    Review -- "reading 1..*" --> Step
    Review -- "edges 0..*" --> Edge
    Review -- "groups 0..*" --> Group
    Definition -- "before 0..1" --> Occurrence
    Definition -- "after 0..1" --> Occurrence
    Occurrence -- "locator 1" --> Locator
    Occurrence -- "parts 1..*" --> Piece
    Edge -- "from 1" --> Definition
    Edge -- "to 1" --> Definition
    Step -- "definition 1" --> Definition
    Group -- "children 0..*" --> Group
    Group -- "node 0..1" --> Definition
```

A definition has an occurrence in the before snapshot, the after snapshot, or both. Groups nest in one way only: a group from `[group]` in `dagger.toml` holds the definitions under it, and a definition that holds other definitions, such as a module, impl, or class, is a group of its own inside that, with a node for itself when it is read. Two groups from markers never contain each other.

## Glossary

| term              | meaning                                                                                                                                                                                                                                                                |
| ----------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| adapter           | a program dagger runs and talks to over JSON. There are two kinds: the snapshot adapter and extractor adapters. Both are named by `adapter =` in `dagger.toml`.                                                                                                        |
| before / after    | the two snapshots being compared, and which snapshot anything that spans them was seen in (`Sides::Kept`, `Reference::before` / `after`).                                                                                                                              |
| body              | the part callers cannot see. Changing it can't break callers.                                                                                                                                                                                                          |
| change            | what happened to one definition between the snapshots: `Change::Added`, `Removed`, or `Kept(Edits)`. Held on `review::Definition`.                                                                                                                                     |
| contract          | the part callers can see, including the name. `Occurrence::contract_from_compiler` is the compiler's own view of it and, when present in both snapshots, decides whether callers broke instead of the written text. `Edits::contract_changed` says it changed.                               |
| definition        | one named thing in the source. `model::Definition` is its two occurrences under one identity. `review::Definition` is everything the review knows about it.                                                                                                            |
| docs              | the part written for callers that can't break them.                                                                                                                                                                                                                    |
| edge              | one shown definition depending on another (`Edge { from, to }`), with unshown definitions stepped over. Held on `Review::edges`.                                                                                                                                       |
| extractor adapter | an adapter that reads one snapshot and reports an `Extraction`: occurrences and mentions for its assigned files. Configured under `[[extractors]]`.                                                                                                                    |
| group             | one box on the page (`Group::Group`: name, tier, children) or one thing in it (`Group::Node`). Boxes come from `[group]` and from containers. Held on `Review::groups`.                                                                                                |
| grouping          | the one way of grouping definitions in use, like "package" or "crate": a name and each identity's path (`Grouping`). Configured by `[group]` in `dagger.toml`.                                                                                                         |
| identity          | the number matching hands a definition so both snapshots share it (`Identity`). Means nothing on its own.                                                                                                                                                              |
| locator           | where a definition lives in one snapshot: scope plus name (`Locator`). The name is kept apart because renaming breaks callers and moving doesn't.                                                                                                                      |
| mention           | one spot where a definition mentions another in a single snapshot (`Mention { from, to, site }`), as far as an extractor adapter can get.                                                                                                                              |
| occurrence        | a definition as it exists in one snapshot (`Occurrence`): locator, role, parent, kind, file, parts, contract from the compiler.                                                                                                                                                          |
| on faith          | a dependency read after the thing that depends on it, which only happens in a cycle. Listed per `Step::on_faith`, counted in `Cost::taken_on_faith`.                                                                                                                   |
| open              | a definition already read while something unread still depends on it. Counted in `Cost::peak_open` and `total_open`.                                                                                                                                                   |
| part              | one of the kinds a definition's text is split into: `Type`, `Body`, `Docs` (`Part`). A part can be missing when it doesn't apply.                                                                                                                                      |
| piece             | one stretch of source belonging to a part (`Piece`: text, span, line, file). A part is a list of these because its source isn't always contiguous.                                                                                                                     |
| reached           | hops from the nearest change that reached a definition through references (`review::Definition::reached`). Never zero: its own change is `change`.                                                                                                                     |
| reading           | what to read, in order (`Review::reading`, a list of steps). Whatever a step names is worth reading. Everything else is drawn around it.                                                                                                                               |
| ripples           | how far a change is followed outward through references, in hops (`Review::ripples`, `Request::Extract::ripples`, `ReviewConfig::ripples`).                                                                                                                            |
| role              | whether a definition can hold others (`Role`): `Item` reads on its own, `Container` can hold others and is drawn as a group. Said by the extractor adapter.                                                                                                            |
| sides             | which snapshots a definition showed up in (`Sides`): `Added`, `Removed`, or `Kept { before, after }`.                                                                                                                                                                  |
| site              | one spot in the source where a mention sits: part, span, and which extractor found it (`Site`).                                                                                                                                                                           |
| snapshot          | one version of the code, laid out on disk by the snapshot adapter, which answers `Resolve` and `Materialize`.                                                                                                                                                          |
| snapshot adapter  | the adapter that turns what the user asked for into two snapshots on disk. Configured under `[snapshots]`.                                                                                                                                                             |
| step              | one entry in the reading: a definition plus what was taken on faith to read it (`Step`).                                                                                                                                                                               |
| target            | what a mention or reference points at (`Target`): `Known(T)`, or `Unknown { symbol }` when nothing could bind it.                                                                                                                                                      |
| tier              | a group's or node's depth among its siblings: zero if it depends on nothing else in its holder, else one more than its deepest dependency. Held on `Group`.                                                                                                            |
| warning           | a reason the review might be wrong, worded for the reader (`Warning`: impact, message, about). Adapter notes and dagger's own diagnostics share the list. `Impact::Incomplete` means something may be missing. `Degraded` means it's shown but from worse information. |
