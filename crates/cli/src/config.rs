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
    pub group: Option<GroupConfig>,
    #[serde(default)]
    pub review: ReviewConfig,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GroupConfig {
    /// What a reader would call it: "package", "crate", "service".
    pub name: String,
    /// Files whose directory is a group: `BUILD.bazel`, `Cargo.toml`, `package.json`. A
    /// definition belongs to the nearest marker above its file. Several kinds, because one
    /// repository is often several kinds of thing at once.
    pub markers: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Adapter {
    /// A command. `git`, `lsp` and `rust` are the adapters built into dagger itself;
    /// otherwise a slash makes it a path in the repo, anything else comes off PATH.
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
    /// Globs this extractor adapter claims. The last one listed wins a contested file, so a
    /// broad rule can be narrowed by a later one. Unclaimed files are read whole.
    #[serde(default)]
    pub include: Vec<String>,
    /// Set when dagger picked this extractor itself rather than reading it from the file.
    #[serde(skip)]
    pub inferred: bool,
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
        let written = match std::fs::read_to_string(&path) {
            Ok(text) => {
                // The parse error goes in the message itself, since it's the actionable part.
                toml::from_str(&text)
                    .map_err(|why| anyhow::anyhow!("{} doesn't parse: {}", path.display(), why))?
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Self::default(),
            Err(error) => {
                return Err(error).with_context(|| format!("couldn't read {}", path.display()));
            }
        };
        Ok(inferred(repo, written))
    }
}

/// Fills in what wasn't configured from what the repository visibly is, so a repository
/// with no dagger.toml still gets its git history and its languages read.
fn inferred(repo: &Path, config: Config) -> Config {
    let built_in = |adapter: &str, include: &[&str], settings: toml::Value| Extractor {
        adapter: adapter.to_string(),
        args: Vec::new(),
        settings,
        include: include.iter().map(|glob| glob.to_string()).collect(),
        inferred: true,
    };

    let snapshots = config.snapshots.or_else(|| {
        repo.join(".git").exists().then(|| Adapter {
            adapter: "git".to_string(),
            args: Vec::new(),
            settings: nothing(),
        })
    });

    let extractors = if config.extractors.is_empty() {
        let rust = repo
            .join("Cargo.toml")
            .exists()
            .then(|| built_in("rust", &[], nothing()));
        let typescript = (repo.join("tsconfig.json").exists()
            || repo.join("package.json").exists())
        .then(|| {
            let settings = toml::toml! { server = ["tsc", "--lsp", "--stdio"] };
            built_in(
                "lsp",
                &["**/*.ts", "**/*.tsx"],
                toml::Value::Table(settings),
            )
        });
        rust.into_iter().chain(typescript).collect()
    } else {
        config.extractors
    };

    Config {
        snapshots,
        extractors,
        ..config
    }
}
