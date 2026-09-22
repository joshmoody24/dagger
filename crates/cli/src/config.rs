use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::Path;

pub const FILE: &str = "dagger.toml";

/// How one repo wants its reviews built. Absent entirely is fine: every file then
/// falls to the built-in whole-file reading, which is coarse but honest.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub snapshots: Option<Adapter>,
    #[serde(default)]
    pub extractors: Vec<Extractor>,
    /// How to group the files, if they should be grouped at all.
    ///
    /// One, not a list. Boxes have to nest to be drawn and two groupings of the same code
    /// rarely nest inside one another, so only one was ever used — a repository could write
    /// down three and dagger would quietly read the first. A setting that accepts more than
    /// it obeys is a setting that lies.
    pub grouping: Option<GroupingConfig>,
    #[serde(default)]
    pub review: ReviewConfig,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GroupingConfig {
    /// What a reader would call it: "package", "crate", "service".
    pub name: String,
    /// Files whose nearest containing directory is the group. `BUILD.bazel` for a bazel
    /// package, `Cargo.toml` for a crate, `package.json` for a workspace package.
    ///
    /// Several, because one repository is often several kinds of thing at once — this one
    /// is Rust crates with a TypeScript package inside it — and a reader wants each drawn
    /// as whatever it actually is. The nearest marker of any kind wins, so they still nest.
    pub markers: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Adapter {
    /// A command, read the way a shell would: a slash makes it a path in the repo,
    /// anything else comes off PATH.
    pub adapter: String,
    #[serde(default)]
    pub args: Vec<String>,
    /// Passed through untouched. Anything an adapter needs telling belongs here rather
    /// than in `args`, which stays what it looks like: arguments to a program.
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
    /// The files this extractor speaks for. When two extractors claim the same file
    /// the last one listed wins, so a broad rule can be narrowed by a later one.
    /// Whatever no extractor claims falls back to being read whole.
    #[serde(default)]
    pub include: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewConfig {
    /// Files to leave out of the review altogether, whoever would have claimed them.
    /// The one place completeness is given up on purpose, so it's worth keeping short.
    #[serde(default)]
    pub ignore: Vec<String>,
    /// How far past what changed to follow what depends on it, when nobody says.
    ///
    /// A repository knows things about itself that a default can't: how widely its pieces
    /// are used, how long its tooling takes to answer, whether following a change outward
    /// is a second's work or a minute's. `--ripples` still wins, so this is where a repo
    /// starts from rather than what it insists on.
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
                /* The reason comes first, because it's the part anybody can act on: a
                 * misspelled key, a string where a list belongs. Left as context the error
                 * said only that the file doesn't parse, which is the one thing already
                 * obvious from being told at all. */
                toml::from_str(&text)
                    .map_err(|why| anyhow::anyhow!("{} doesn't parse: {}", path.display(), why))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error).with_context(|| format!("couldn't read {}", path.display())),
        }
    }
}
