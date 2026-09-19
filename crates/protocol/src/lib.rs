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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum Response {
    Materialized {
        dir: String,
        /// Whether dagger should delete the directory when it's done. An adapter that
        /// pointed at a path it doesn't own says false.
        temporary: bool,
    },
    Extracted(Extraction),
    /// Something went wrong that the user should hear about, worded for them.
    Failed {
        message: String,
    },
}
