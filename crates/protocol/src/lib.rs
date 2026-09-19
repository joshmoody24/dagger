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
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Request {
    /// What this adapter can do, asked before anything else. An extractor answers
    /// with the files it speaks for, so a repo doesn't have to spell out that a Rust
    /// adapter reads Rust.
    Describe,
    /// Put this revision somewhere on disk and say where.
    Materialize { rev: String },
    /// Read a snapshot and report what's defined in the files handed over.
    ///
    /// These are every file the adapter owns, not just the ones that changed: an
    /// untouched helper can still be what joins two edits together. Reading other
    /// files for context is fine and often necessary, but report only these, since
    /// dagger has already decided who speaks for what.
    Extract { dir: String, files: Vec<String> },
}

/// Something an adapter wants the reader to know: a file it couldn't parse, a
/// project it couldn't make sense of. Prose rather than a fixed set of cases,
/// because dagger can't know in advance what a given language's tooling will run
/// into.
#[derive(Debug, Clone, Serialize, Deserialize)]
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
    Failed { message: String },
}
