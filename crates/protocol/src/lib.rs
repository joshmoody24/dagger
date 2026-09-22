//! What dagger says to an adapter, and what it expects back.
//!
//! An adapter is any executable: one JSON request on stdin, one JSON response on stdout,
//! and stderr reaches the user. Snapshots are handed over as a directory because real
//! language tooling wants a project on disk.

use dagger_core::matching::Extraction;
use dagger_core::model::Span;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Request {
    /// What this adapter can do, asked before anything else: the files it speaks for, and
    /// for a snapshot adapter, which revisions to compare by default.
    Describe {
        /// Whatever the repo wrote under this adapter's `settings`. Dagger doesn't read it.
        #[serde(default)]
        settings: serde_json::Value,
    },
    /// Turn the user's command-line arguments into the two revisions to compare. They
    /// travel exactly as typed, since naming history (`main...HEAD`) is the adapter's business.
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
    /// Read a snapshot and report what's defined in `files`: every file the adapter owns,
    /// not just the changed ones. Reading other files for context is fine; report only these.
    Extract {
        dir: String,
        files: Vec<String>,
        /// Files that differ between the snapshots, and where. A hint, not a filter: an
        /// adapter may still report anything, and ignoring this is only slow, not wrong.
        /// The ranges are for this side (`dir`) only.
        #[serde(default)]
        changed: Vec<Changed>,
        /// How far past a changed file to follow what uses it; zero means not at all. A
        /// hint like `changed`, and the one that decides what a reading costs.
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
    /// What the commit under review is called, when the adapter can say. Only ever a
    /// suggestion, never required.
    #[serde(default)]
    pub title: Option<String>,
}

/// One file that differs, and the byte ranges inside it that do. The ranges let an
/// adapter tell a touched file from a rewritten one instead of asking about everything.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Changed {
    pub file: String,
    /// Where, in this file, on this side. Empty means the whole file counts — there's
    /// nothing narrower to say about a file that only exists on one side of the change.
    pub at: Vec<Span>,
}

/// What an adapter says on stderr while a reading is under way. A fixed vocabulary keeps
/// the wording the same across adapters.
#[derive(Debug, Clone, PartialEq)]
pub enum Progress {
    /// A server was started to answer what's ahead.
    StartingServer { name: String },
    /// The server is indexing before it can be trusted to answer anything.
    Indexing { name: String },
    /// How far a walk outward from what changed has gotten.
    Walked {
        done: usize,
        known: usize,
        opened: usize,
    },
    /// What a reading found, with nothing left to ask.
    Finished { files: usize, definitions: usize },
}

impl std::fmt::Display for Progress {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Progress::StartingServer { name } => write!(f, "  starting {name}"),
            Progress::Indexing { name } => write!(f, "  waiting for {name} to index"),
            Progress::Walked {
                done,
                known,
                opened,
            } => write!(f, "  walked {done} of {known} files, opened {opened}"),
            Progress::Finished { files, definitions } => {
                write!(f, "  read {files} files, found {definitions} definitions")
            }
        }
    }
}

/// Something an adapter wants the reader to know, like a file it couldn't parse. Prose
/// rather than fixed cases, because dagger can't know what a language's tooling will run into.
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
        /// What to compare when the user named nothing. Only a snapshot adapter knows,
        /// since only it knows whether there's uncommitted work around.
        #[serde(default)]
        revisions: Option<Revisions>,
        /// The ways of naming a change this adapter accepts, a line each, for dagger's
        /// help text. Kept here so there's only one copy of the list.
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
        /// Which files are worth reading, if the adapter knows. Left out means look
        /// yourself. A version control adapter knows, which keeps ignored files out.
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

#[cfg(test)]
mod progress_tests {
    use super::Progress;

    #[test]
    fn rendering_matches_what_adapters_used_to_write_by_hand() {
        assert_eq!(
            Progress::StartingServer {
                name: "rust-analyzer".to_string()
            }
            .to_string(),
            "  starting rust-analyzer"
        );
        assert_eq!(
            Progress::Indexing {
                name: "rust-analyzer".to_string()
            }
            .to_string(),
            "  waiting for rust-analyzer to index"
        );
        assert_eq!(
            Progress::Walked {
                done: 3,
                known: 5,
                opened: 7
            }
            .to_string(),
            "  walked 3 of 5 files, opened 7"
        );
        assert_eq!(
            Progress::Finished {
                files: 8,
                definitions: 141
            }
            .to_string(),
            "  read 8 files, found 141 definitions"
        );
    }
}
