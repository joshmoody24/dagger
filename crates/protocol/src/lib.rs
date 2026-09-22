//! What dagger says to an adapter, and what it expects back.
//!
//! An adapter is any executable. It reads one request as JSON on stdin, writes one
//! response as JSON on stdout, and exits. Whatever it puts on stderr reaches the
//! user, so logging there is fine.
//!
//! Snapshots are handed over as a directory rather than served a file at a time,
//! because real language tooling wants a project on disk: a tsconfig, a lockfile,
//! the imports next door.

use dagger_core::matching::Extraction;
use dagger_core::model::Span;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Request {
    /// What this adapter can do, asked before anything else. An extractor answers with
    /// the files it speaks for, so a repo doesn't have to spell out that a Rust adapter
    /// reads Rust. A snapshot adapter can also say which two revisions to compare when
    /// the user hasn't named any.
    Describe {
        /// Whatever the repo wrote under this adapter's `settings`. Dagger doesn't read
        /// it: an adapter that needs to be told which server to run, or which dialect to
        /// expect, gets told here rather than through a pile of arguments.
        #[serde(default)]
        settings: serde_json::Value,
    },
    /// Turn what the user asked for on the command line into the two revisions to
    /// compare.
    ///
    /// The words are the adapter's, not dagger's. "Branch", "commits", `main...HEAD` —
    /// all of that is git's way of naming history, and a repository kept some other way
    /// names it some other way. So the arguments travel exactly as typed and whoever
    /// understands them says what they meant.
    Resolve {
        /// Everything after the flags, in order, untouched.
        asked: Vec<String>,
        #[serde(default)]
        settings: serde_json::Value,
    },
    /// Put this revision somewhere on disk and say where.
    Materialize {
        rev: String,
        #[serde(default)]
        settings: serde_json::Value,
    },
    /// Read a snapshot and report what's defined in the files handed over.
    ///
    /// These are every file the adapter owns, not just the ones that changed: an
    /// untouched helper can still be what joins two edits together. Reading other
    /// files for context is fine and often necessary, but report only these, since
    /// dagger has already decided who speaks for what.
    Extract {
        dir: String,
        files: Vec<String>,
        /// Files that differ between the two snapshots, and where in each. A hint, not a
        /// filter: an adapter may still report anything it likes, and one that ignores
        /// this is merely slow rather than wrong. It lets an adapter work outward from a
        /// change, and ask about only the definitions a change actually touches, rather
        /// than reading a whole repository — or a whole changed file — to describe a few
        /// lines.
        ///
        /// The ranges are for this side of the comparison: `dir` is one snapshot, and a
        /// definition unchanged here can still be worth asking about because the other
        /// snapshot moved it. What moved on the other side isn't this request's to say.
        #[serde(default)]
        changed: Vec<Changed>,
        /// How far past a changed file to follow what uses it. Nought means not at all.
        ///
        /// A hint like `changed`, and the one that decides what a reading costs: each hop
        /// outward is every definition reached so far asking the whole repository who uses
        /// it. An adapter that reads whole files regardless has nothing to do with this.
        #[serde(default)]
        ripples: u32,
        #[serde(default)]
        settings: serde_json::Value,
    },
}

/// Two revisions to compare.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Revisions {
    pub before: String,
    pub after: String,
}

/// One file that differs, and the byte ranges inside it that do.
///
/// A whole file used to count as changed the moment one line in it did, which made a
/// changed file and a rewritten one look the same request: an adapter had no way to tell
/// "ask about everything here" from "ask about the four lines somebody touched", so it
/// asked about everything either way. Redo's own self-review asked after 494 definitions
/// in files that between them had a few dozen lines actually differ.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Changed {
    pub file: String,
    /// Where, in this file, on this side. Empty means the whole file counts — there's
    /// nothing narrower to say about a file that only exists on one side of the change.
    pub at: Vec<Span>,
}

/// Something an adapter wants the reader to know: a file it couldn't parse, a project
/// it couldn't make sense of. Prose rather than a fixed set of cases, because dagger
/// can't know in advance what a given language's tooling will run into.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct Note {
    pub message: String,
    /// The file it's about, when it's about one.
    pub file: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum Response {
    Described {
        /// Glob patterns this adapter claims by default. A repo that says `include`
        /// replaces this outright rather than adding to it.
        include: Vec<String>,
        /// What to compare when the user named nothing. Only a snapshot adapter knows
        /// what a sensible answer is, since only it knows whether there's uncommitted
        /// work sitting around.
        #[serde(default)]
        revisions: Option<Revisions>,
        /// The ways of naming a change this adapter answers to, a line each, for dagger
        /// to show alongside its own help. Written here because the words are this
        /// adapter's: dagger printing them itself would be a second copy of a list only
        /// one of them can keep right.
        #[serde(default)]
        usage: Vec<String>,
    },
    Resolved {
        revisions: Revisions,
    },
    Materialized {
        dir: String,
        /// Whether dagger should delete the directory when it's done. An adapter that
        /// pointed at a path it doesn't own says false.
        temporary: bool,
        /// What's in there worth reading, if the adapter knows. Left out means "have a
        /// look yourself", which is the only option when the directory is just a
        /// directory. A version control adapter does know, and saying so is how
        /// ignored files stay out without every repo listing its build directory.
        #[serde(default)]
        files: Option<Vec<String>>,
    },
    Extracted {
        extraction: Extraction,
        /// Anything that went less than perfectly. An adapter that skipped a file it
        /// couldn't read says so here rather than failing the whole run.
        #[serde(default)]
        notes: Vec<Note>,
    },
    /// Something went wrong that the user should hear about, worded for them.
    Failed {
        message: String,
    },
}
