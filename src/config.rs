//! `docket.ncl` loading and genre path matching (MVP.md §2).

use crate::model::{Kind, Quadrant};
use crate::nickel::{self, NickelError};
use std::path::Path;

/// Where the config contract is expected to live, relative to the current
/// directory — a project-level artifact shared across every corpus root,
/// the same discipline `checks::DEFAULT_REGISTER_RELATIVE_PATH` follows
/// for the register evaluator.
pub const DEFAULT_CONFIG_CONTRACT_RELATIVE_PATH: &str = "contracts/docket.ncl";

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("{path} not found — a corpus needs docket.ncl at its root")]
    Missing { path: String },
    /// Covers both "doesn't evaluate" and "evaluates but fails
    /// `contracts/docket.ncl`" — the contract is the single authority for
    /// the config's shape and its derived rules (genre fields, the
    /// `Kind`/`Quadrant` enums, explanation-forbids-kinds), so Rust does
    /// not re-check any of that and has nothing to add beyond what
    /// `source` already reports.
    #[error("{path}: {source}")]
    Nickel {
        path: String,
        #[source]
        source: NickelError,
    },
    /// Glob syntax is not a Nickel-expressible shape — the contract only
    /// guarantees `path` is a non-empty string; compiling it into a
    /// matchable pattern is I/O-adjacent parsing, same as markdown, and
    /// stays in Rust (MVP.md §7 draws the line at "checking Nickel could
    /// do", not "checking of any kind").
    #[error("{path}: genre path pattern {pattern:?} is not a valid glob: {detail}")]
    InvalidPattern {
        path: String,
        pattern: String,
        detail: String,
    },
}

/// MVP.md §2: "A path matching more than one genre is a configuration
/// error (exit 2), not a precedence question." Raised per corpus-relative
/// path as the walk encounters it (corpus.rs), not by statically
/// analyzing the patterns for overlap — two patterns can overlap in the
/// abstract without any real file ever landing in the intersection.
#[derive(Debug, thiserror::Error)]
#[error("{path} matches more than one genre: {}", matched.join(", "))]
pub struct AmbiguousGenre {
    pub path: String,
    pub matched: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Genre {
    /// The glob pattern as written in `docket.ncl`, kept for diagnostics
    /// and for the index's `genre` field (§4.1 wants the pattern string,
    /// not a synthesized name).
    pub path: String,
    pub kinds: Vec<Kind>,
    /// Which of Divio's four quadrants this genre's documents serve
    /// (MVP.md §2) — required and closed, so a corpus's genre taxonomy is
    /// always comparable to another corpus's.
    pub quadrant: Quadrant,
    compiled: glob::Pattern,
}

#[derive(Debug, Clone, Default)]
pub struct Config {
    pub genres: Vec<Genre>,
}

impl Config {
    /// The genre whose pattern matches `corpus_relative_path`, if any —
    /// `Ok(None)` for "files matching no genre are not scanned" (MVP.md
    /// §2). More than one match is `Err`, not a precedence question:
    /// both plausible tie-break conventions (first-match, most-specific)
    /// surprise somebody, and first-match is the *opposite* of
    /// `.gitignore`'s later-wins rule, so the config is required to be
    /// unambiguous instead.
    pub fn match_genre(
        &self,
        corpus_relative_path: &str,
    ) -> Result<Option<&Genre>, AmbiguousGenre> {
        let matched: Vec<&Genre> = self
            .genres
            .iter()
            .filter(|g| g.compiled.matches(corpus_relative_path))
            .collect();
        match matched.len() {
            0 => Ok(None),
            1 => Ok(Some(matched[0])),
            _ => Err(AmbiguousGenre {
                path: corpus_relative_path.to_string(),
                matched: matched.into_iter().map(|g| g.path.clone()).collect(),
            }),
        }
    }
}

/// Load and validate `<corpus_root>/docket.ncl` against
/// `contracts/docket.ncl` by invoking Nickel — never reimplementing its
/// checking (MVP.md §7). By the time this returns `Ok`, `value` has
/// already satisfied the contract's shape and its derived rules; the loop
/// below only converts trusted JSON into typed `Genre`s and compiles glob
/// patterns, the one piece the contract cannot check.
pub fn load_config(corpus_root: &Path) -> Result<Config, ConfigError> {
    let path = corpus_root.join("docket.ncl");
    let path_str = path.display().to_string();

    if !path.is_file() {
        return Err(ConfigError::Missing { path: path_str });
    }

    let contract_path = Path::new(DEFAULT_CONFIG_CONTRACT_RELATIVE_PATH);
    let value = nickel::export_json_with_contract(&path, contract_path).map_err(|source| {
        ConfigError::Nickel {
            path: path_str.clone(),
            source,
        }
    })?;

    let genres_json = value
        .get("genres")
        .and_then(|g| g.as_array())
        .expect("contracts/docket.ncl guarantees a top-level `genres` array");

    let mut genres = Vec::with_capacity(genres_json.len());
    for entry in genres_json {
        let genre_path = entry
            .get("path")
            .and_then(|v| v.as_str())
            .expect("contracts/docket.ncl guarantees each genre has a non-empty string `path`")
            .to_string();

        let kinds_json = entry
            .get("kinds")
            .and_then(|v| v.as_array())
            .expect("contracts/docket.ncl guarantees each genre has an array `kinds`");
        let kinds: Vec<Kind> = kinds_json
            .iter()
            .map(|k| {
                let k_str = k
                    .as_str()
                    .expect("contracts/docket.ncl guarantees kinds are strings");
                parse_kind(k_str).unwrap_or_else(|| {
                    panic!(
                        "contracts/docket.ncl guarantees kind is requirement/invariant/constraint, got {k_str:?}"
                    )
                })
            })
            .collect();

        let quadrant_str = entry
            .get("quadrant")
            .and_then(|v| v.as_str())
            .expect("contracts/docket.ncl guarantees each genre has a string `quadrant`");
        let quadrant = parse_quadrant(quadrant_str).unwrap_or_else(|| {
            panic!(
                "contracts/docket.ncl guarantees quadrant is tutorial/how-to/reference/explanation, got {quadrant_str:?}"
            )
        });

        let compiled =
            glob::Pattern::new(&genre_path).map_err(|e| ConfigError::InvalidPattern {
                path: path_str.clone(),
                pattern: genre_path.clone(),
                detail: e.to_string(),
            })?;

        genres.push(Genre {
            path: genre_path,
            kinds,
            quadrant,
            compiled,
        });
    }

    Ok(Config { genres })
}

fn parse_kind(s: &str) -> Option<Kind> {
    match s {
        "requirement" => Some(Kind::Requirement),
        "invariant" => Some(Kind::Invariant),
        "constraint" => Some(Kind::Constraint),
        _ => None,
    }
}

fn parse_quadrant(s: &str) -> Option<Quadrant> {
    match s {
        "tutorial" => Some(Quadrant::Tutorial),
        "how-to" => Some(Quadrant::HowTo),
        "reference" => Some(Quadrant::Reference),
        "explanation" => Some(Quadrant::Explanation),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_docket_ncl(dir: &Path, contents: &str) {
        let mut f = std::fs::File::create(dir.join("docket.ncl")).unwrap();
        f.write_all(contents.as_bytes()).unwrap();
    }

    #[test]
    fn loads_the_mvp_example_config() {
        let dir = tempdir();
        write_docket_ncl(
            dir.path(),
            r#"{
              genres = [
                { path = "docs/specs/**", kinds = ["constraint"], quadrant = "reference" },
                { path = "docs/models/**", kinds = ["invariant"], quadrant = "reference" },
                { path = "docs/architecture/**", kinds = ["requirement"], quadrant = "reference" },
                { path = "docs/adr/**", kinds = [], quadrant = "explanation" },
              ],
            }"#,
        );
        let config = load_config(dir.path()).expect("config should load");
        assert_eq!(config.genres.len(), 4);
        assert_eq!(config.genres[0].path, "docs/specs/**");
        assert_eq!(config.genres[0].kinds, vec![Kind::Constraint]);
        assert_eq!(config.genres[0].quadrant, Quadrant::Reference);
        assert!(config.genres[3].kinds.is_empty());
        assert_eq!(config.genres[3].quadrant, Quadrant::Explanation);
    }

    #[test]
    fn missing_docket_ncl_is_a_config_error() {
        let dir = tempdir();
        let err = load_config(dir.path()).unwrap_err();
        assert!(matches!(err, ConfigError::Missing { .. }));
    }

    #[test]
    fn unknown_kind_is_a_config_error() {
        let dir = tempdir();
        write_docket_ncl(
            dir.path(),
            r#"{ genres = [ { path = "docs/**", kinds = ["opinion"], quadrant = "reference" } ] }"#,
        );
        let err = load_config(dir.path()).unwrap_err();
        assert!(matches!(err, ConfigError::Nickel { .. }));
        assert!(err.to_string().contains("kinds"));
    }

    #[test]
    fn unknown_quadrant_is_a_config_error() {
        let dir = tempdir();
        write_docket_ncl(
            dir.path(),
            r#"{ genres = [ { path = "docs/**", kinds = ["constraint"], quadrant = "opinion" } ] }"#,
        );
        let err = load_config(dir.path()).unwrap_err();
        assert!(matches!(err, ConfigError::Nickel { .. }));
        assert!(err.to_string().contains("quadrant"));
    }

    #[test]
    fn missing_quadrant_is_a_config_error() {
        let dir = tempdir();
        write_docket_ncl(
            dir.path(),
            r#"{ genres = [ { path = "docs/**", kinds = ["constraint"] } ] }"#,
        );
        let err = load_config(dir.path()).unwrap_err();
        assert!(matches!(err, ConfigError::Nickel { .. }));
        assert!(err.to_string().contains("quadrant"));
    }

    #[test]
    fn explanation_with_nonempty_kinds_is_a_config_error() {
        // The derived rule (MVP.md §2): explanation carries rationale, not
        // checkable claims — a genre claiming both is a contradiction in
        // docket.ncl itself, caught at config-load time rather than
        // treated as a corpus-content check. Enforced by
        // contracts/docket.ncl's `GenresValid` validator now, not by Rust.
        let dir = tempdir();
        write_docket_ncl(
            dir.path(),
            r#"{ genres = [ { path = "docs/adr/**", kinds = ["requirement"], quadrant = "explanation" } ] }"#,
        );
        let err = load_config(dir.path()).unwrap_err();
        assert!(matches!(err, ConfigError::Nickel { .. }));
        let message = err.to_string();
        assert!(message.contains("docs/adr/**"));
        assert!(message.contains("explanation carries rationale, not checkable claims"));
    }

    #[test]
    fn invalid_glob_pattern_is_a_config_error() {
        // Glob syntax is not a Nickel-expressible shape (the contract
        // only guarantees `path` is a non-empty string), so this stays a
        // Rust-side check — the one piece load_config still performs
        // itself after the contract has validated everything else.
        let dir = tempdir();
        write_docket_ncl(
            dir.path(),
            r#"{ genres = [ { path = "docs/[", kinds = ["constraint"], quadrant = "reference" } ] }"#,
        );
        let err = load_config(dir.path()).unwrap_err();
        assert!(matches!(err, ConfigError::InvalidPattern { .. }));
    }

    #[test]
    fn explanation_with_empty_kinds_loads_cleanly() {
        let dir = tempdir();
        write_docket_ncl(
            dir.path(),
            r#"{ genres = [ { path = "docs/adr/**", kinds = [], quadrant = "explanation" } ] }"#,
        );
        let config = load_config(dir.path()).expect("kinds = [] under explanation is legal");
        assert_eq!(config.genres[0].quadrant, Quadrant::Explanation);
    }

    #[test]
    fn match_genre_matches_a_glob_recursive_pattern() {
        let dir = tempdir();
        write_docket_ncl(
            dir.path(),
            r#"{ genres = [ { path = "docs/specs/**", kinds = ["constraint"], quadrant = "reference" } ] }"#,
        );
        let config = load_config(dir.path()).unwrap();
        assert!(
            config
                .match_genre("docs/specs/lock-file-schema.md")
                .unwrap()
                .is_some()
        );
        assert!(
            config
                .match_genre("docs/specs/nested/deep.md")
                .unwrap()
                .is_some()
        );
        assert!(config.match_genre("docs/models/x.md").unwrap().is_none());
        assert!(config.match_genre("README.md").unwrap().is_none());
    }

    #[test]
    fn a_path_matching_more_than_one_genre_is_an_error() {
        let dir = tempdir();
        write_docket_ncl(
            dir.path(),
            r#"{
              genres = [
                { path = "docs/**", kinds = ["requirement"], quadrant = "reference" },
                { path = "docs/specs/**", kinds = ["constraint"], quadrant = "reference" },
              ],
            }"#,
        );
        let config = load_config(dir.path()).unwrap();
        let err = config.match_genre("docs/specs/x.md").unwrap_err();
        assert_eq!(err.path, "docs/specs/x.md");
        assert_eq!(err.matched, vec!["docs/**", "docs/specs/**"]);
    }

    /// A minimal, dependency-free temp dir: create under the system temp
    /// dir, remove on drop. `tempfile`/`tempdir` crates are unnecessary
    /// for a handful of tests.
    struct TempDir(std::path::PathBuf);
    impl TempDir {
        fn path(&self) -> &Path {
            &self.0
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn tempdir() -> TempDir {
        let dir = std::env::temp_dir().join(format!(
            "docket-config-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
}
