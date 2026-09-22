# Dagger's data model

This is the vocabulary dagger uses and how the words relate. The Rust types in
`crates/core/src` are the source of truth.

## A review, end to end

A review is one reading of the difference between two code snapshots. Each word below is
defined where the tool first produces the thing it names.

1. A **snapshot** is one version of the code, laid out on disk. The snapshot adapter turns
   what the user asked for (`main...HEAD`, or nothing) into two snapshots, **before** and
   **after**.
2. In each snapshot, the extractor adapters read the files they claim. For every named thing in a
   file they report an **occurrence**: where it is (a **locator**, the file and a path of
   names) and what it is made of. What it is made of is a list of **pieces**, each a stretch
   of text that belongs to one **part**: the **contract**, which callers can see; the
   **body**, which they can't; or the **docs**. Beside the occurrences, the extractor adapters
   report **mentions**: places where one occurrence names another.
3. The core matches the occurrences of the two snapshots. An occurrence in the before
   snapshot and one in the after snapshot that are the same named thing become one
   **definition**, given an **identity**. A definition with an occurrence in only one snapshot
   is inferred to be "added" or "removed". One with both is "kept", and its **change** says
   which parts differ.
4. Mentions become **references** between definitions, and the core follows them outward
   from what changed, as many steps as the **ripples** setting allows, to find what a change
   **reached**. The references among the definitions shown become **edges**: what depends
   on what.
5. The core decides the **reading**: a list of **steps**, one definition each, in the order
   to read them, nothing before what it depends on.
6. The core shapes the definitions into **groups**, the boxes on the page, each with a
   **tier**: how many rows down from what it depends on it sits.
7. The page in the browser, or the terminal, renders that one **review** and decides
   nothing else.

## The core model

A `Review` is what one reading of a change comes to. Everything hangs off its definitions.

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

| term | meaning |
|---|---|
| adapter | a program dagger runs and talks to over JSON. There are two kinds: the snapshot adapter and extractor adapters. Both are named by `adapter =` in `dagger.toml`. |
| before / after | the two snapshots being compared, and which snapshot anything that spans them was seen in (`Sides::Kept`, `Reference::before` / `after`). |
| body | the part that is internal. Changing it can't break callers. |
| change | what happened to one definition between the snapshots: `Change::Added`, `Removed`, or `Kept(Edits)`. Held on `review::Definition`. |
| contract | the `Type` part: what callers see, including the name. `Occurrence::contract` is the compiler's own view of it and, when present in both snapshots, decides whether callers broke instead of the written text. `Edits::contract` says it changed. |
| definition | one named thing in the source. `model::Definition` is its two occurrences under one identity. `review::Definition` is everything the review knows about it. |
| docs | the part written for callers that can't break them. |
| edge | one shown definition depending on another (`Edge { from, to }`), with unshown definitions stepped over. Held on `Review::edges`. |
| extractor adapter | an adapter that reads one snapshot and reports an `Extraction`: occurrences and mentions for the files it claims. Configured under `[[extractors]]`. |
| group | one box on the page (`Group::Group`: name, tier, children) or one thing in it (`Group::Node`). Boxes come from `[group]` and from containers. Held on `Review::groups`. |
| grouping | the one way of grouping definitions in use, like "package" or "crate": a name and each identity's path (`Grouping`). Configured by `[group]` in `dagger.toml`. |
| identity | the number matching hands a definition so both snapshots share it (`Identity`). Means nothing on its own. |
| locator | where a definition lives in one snapshot: scope plus name (`Locator`). The name is kept apart because renaming breaks callers and moving doesn't. |
| mention | one spot where a definition mentions another in a single snapshot (`Mention { from, to, site }`), as far as an extractor adapter can get. |
| occurrence | a definition as it exists in one snapshot (`Occurrence`): locator, role, parent, kind, file, parts, contract. |
| on faith | a dependency read after the thing that depends on it, which only happens in a cycle. Listed per `Step::on_faith`, counted in `Cost::taken_on_faith`. |
| open | a definition already read while something unread still depends on it. Counted in `Cost::peak_open` and `total_open`. |
| part | one of the kinds a definition's text is split into: `Type`, `Body`, `Docs` (`Part`). A part can be missing when it doesn't apply. |
| piece | one stretch of source belonging to a part (`Piece`: text, span, line, file). A part is a list of these because its source isn't always contiguous. |
| reached | hops from the nearest change that reached a definition through references (`review::Definition::reached`). Never zero: its own change is `change`. |
| reading | what to read, in order (`Review::reading`, a list of steps). Whatever a step names is worth reading. Everything else is drawn around it. |
| ripples | how far a change is followed outward through references, in hops (`Review::ripples`, `Request::Extract::ripples`, `ReviewConfig::ripples`). |
| role | whether a definition can hold others (`Role`): `Item` reads on its own, `Container` can hold others and is drawn as a group. Said by the extractor adapter. |
| sides | which snapshots a definition showed up in (`Sides`): `Added`, `Removed`, or `Kept { before, after }`. |
| site | one spot in the source where a mention sits: part, span, and which binder found it (`Site`). |
| snapshot | one version of the code, laid out on disk by the snapshot adapter, which answers `Resolve` and `Materialize`. |
| snapshot adapter | the adapter that turns what the user asked for into two snapshots on disk. Configured under `[snapshots]`. |
| step | one entry in the reading: a definition plus what was taken on faith to read it (`Step`). |
| target | what a mention or reference points at (`Target`): `Known(T)`, or `Unknown { symbol }` when nothing could bind it. |
| tier | a group's or node's depth among its siblings: zero if it depends on nothing else in its holder, else one more than its deepest dependency. Held on `Group`. |
| warning | a reason the review might be wrong, worded for the reader (`Warning`: impact, message, about). Adapter notes and dagger's own diagnostics share the list. `Impact::Incomplete` means something may be missing. `Degraded` means it's shown but from worse information. |

## Where each thing is decided

- snapshot adapter: which two snapshots are compared, what they're called, and where each sits on disk.
- extractor adapters: what counts as a definition, how its text splits into parts and pieces, its role and parent, and every mention. Only they know the line numbers and the compiler's contract.
- core, matching: which occurrences in the two snapshots are the same definition. Hands out identities and turns mentions into references.
- core, classification and propagation: what changed about each definition and whether it breaks callers, and how far a change reaches through references, up to the ripple depth.
- core, ordering and shape: the reading order and its cost, and how groups nest and which tier each sits on. Decided once so the reading and the page can't disagree.
- page and terminal: pixels only. They draw the review and decide nothing about it.
