use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::Path;

pub const FILE: &str = "dagger.toml";

/// Per-repo settings. Absent, every file falls to whole-file reading.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub snapshots: Option<Adapter>,
    #[serde(default)]
    pub extractors: Vec<Extractor>,
    /// One grouping, not a list: drawn boxes have to nest, and two groupings of the same
    /// code rarely nest inside one another.
    pub grouping: Option<GroupingConfig>,
    #[serde(default)]
    pub review: ReviewConfig,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GroupingConfig {
    /// What a reader would call it: "package", "crate", "service".
    pub name: String,
    /// Files whose nearest containing directory is the group: `BUILD.bazel`, `Cargo.toml`,
    /// `package.json`. Several because one repository is often several kinds of thing at
    /// once; the nearest marker of any kind wins, so they still nest.
    pub markers: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Adapter {
    /// A command: a slash makes it a path in the repo, anything else comes off PATH.
    pub adapter: String,
    #[serde(default)]
    pub args: Vec<String>,
    /// Passed through untouched. Adapter configuration belongs here, not in `args`.
    #[serde(default = "nothing")]
    pub settings: toml::Value,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Extractor {
    pub adapter: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default = "nothing")]
    pub settings: toml::Value,
    /// Globs this extractor claims. The last extractor listed wins a contested file, so a
    /// broad rule can be narrowed by a later one. Unclaimed files are read whole.
    #[serde(default)]
    pub include: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewConfig {
    /// Files left out of the review altogether. The one place completeness is given up
    /// on purpose, so keep it short.
    #[serde(default)]
    pub ignore: Vec<String>,
    /// The repository's default ripple depth. `--ripples` still wins.
    pub ripples: Option<u32>,
}

fn nothing() -> toml::Value {
    toml::Value::Table(toml::map::Map::new())
}

impl Config {
    pub fn read(repo: &Path) -> Result<Self> {
        let path = repo.join(FILE);
        match std::fs::read_to_string(&path) {
            Ok(text) => {
                // The parse error goes in the message itself, since it's the actionable part.
                toml::from_str(&text)
                    .map_err(|why| anyhow::anyhow!("{} doesn't parse: {}", path.display(), why))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error).with_context(|| format!("couldn't read {}", path.display())),
        }
    }
}
