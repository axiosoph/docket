//! `docket.ncl` loading and genre path matching (MVP.md §2).

use crate::model::Kind;
use crate::nickel::{self, NickelError};
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("{path} not found — a corpus needs docket.ncl at its root")]
    Missing { path: String },
    #[error("failed to evaluate {path}: {source}")]
    Nickel {
        path: String,
        #[source]
        source: NickelError,
    },
    #[error("{path}: expected a `genres` array of {{ path, kinds }} records: {detail}")]
    Shape { path: String, detail: String },
    #[error("{path}: genre kind {kind:?} is not one of requirement, invariant, constraint")]
    UnknownKind { path: String, kind: String },
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

/// Load and validate `<corpus_root>/docket.ncl` by invoking Nickel — never
/// reimplementing its checking (MVP.md §7).
pub fn load_config(corpus_root: &Path) -> Result<Config, ConfigError> {
    let path = corpus_root.join("docket.ncl");
    let path_str = path.display().to_string();

    if !path.is_file() {
        return Err(ConfigError::Missing { path: path_str });
    }

    let value = nickel::export_json(&path).map_err(|source| ConfigError::Nickel {
        path: path_str.clone(),
        source,
    })?;

    let genres_json = value
        .get("genres")
        .and_then(|g| g.as_array())
        .ok_or_else(|| ConfigError::Shape {
            path: path_str.clone(),
            detail: "missing or non-array top-level `genres` field".to_string(),
        })?;

    let mut genres = Vec::with_capacity(genres_json.len());
    for entry in genres_json {
        let genre_path = entry
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ConfigError::Shape {
                path: path_str.clone(),
                detail: "a genre entry is missing a string `path`".to_string(),
            })?
            .to_string();

        let kinds_json = entry
            .get("kinds")
            .and_then(|v| v.as_array())
            .ok_or_else(|| ConfigError::Shape {
                path: path_str.clone(),
                detail: format!("genre {genre_path:?} is missing an array `kinds`"),
            })?;

        let mut kinds = Vec::with_capacity(kinds_json.len());
        for k in kinds_json {
            let k_str = k.as_str().ok_or_else(|| ConfigError::Shape {
                path: path_str.clone(),
                detail: format!("genre {genre_path:?} has a non-string entry in `kinds`"),
            })?;
            kinds.push(parse_kind(k_str).ok_or_else(|| ConfigError::UnknownKind {
                path: path_str.clone(),
                kind: k_str.to_string(),
            })?);
        }

        let compiled =
            glob::Pattern::new(&genre_path).map_err(|e| ConfigError::InvalidPattern {
                path: path_str.clone(),
                pattern: genre_path.clone(),
                detail: e.to_string(),
            })?;

        genres.push(Genre {
            path: genre_path,
            kinds,
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
                { path = "docs/specs/**", kinds = ["constraint"] },
                { path = "docs/models/**", kinds = ["invariant"] },
                { path = "docs/architecture/**", kinds = ["requirement"] },
                { path = "docs/adr/**", kinds = [] },
              ],
            }"#,
        );
        let config = load_config(dir.path()).expect("config should load");
        assert_eq!(config.genres.len(), 4);
        assert_eq!(config.genres[0].path, "docs/specs/**");
        assert_eq!(config.genres[0].kinds, vec![Kind::Constraint]);
        assert!(config.genres[3].kinds.is_empty());
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
            r#"{ genres = [ { path = "docs/**", kinds = ["opinion"] } ] }"#,
        );
        let err = load_config(dir.path()).unwrap_err();
        assert!(matches!(err, ConfigError::UnknownKind { .. }));
    }

    #[test]
    fn match_genre_matches_a_glob_recursive_pattern() {
        let dir = tempdir();
        write_docket_ncl(
            dir.path(),
            r#"{ genres = [ { path = "docs/specs/**", kinds = ["constraint"] } ] }"#,
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
                { path = "docs/**", kinds = ["requirement"] },
                { path = "docs/specs/**", kinds = ["constraint"] },
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
