//! Walk a corpus root, match each file against `docket.ncl`'s genres, and
//! extract claims from the ones that match. "Files matching no genre are
//! not scanned" (MVP.md §2).

use crate::config::Config;
use crate::extract::{self, OrphanClaim};
use crate::model::{Corpus, Document};
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum CorpusError {
    #[error("could not walk {path}: {source}")]
    Walk {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("could not read {path} as UTF-8 text: {source}")]
    Read {
        path: String,
        #[source]
        source: std::io::Error,
    },
}

pub struct LoadedCorpus {
    pub corpus: Corpus,
    pub orphan_claims: Vec<OrphanClaim>,
}

/// Load every genre-matched file under `corpus_root`.
pub fn load_corpus(corpus_root: &Path, config: &Config) -> Result<LoadedCorpus, CorpusError> {
    let mut corpus = Corpus::default();
    let mut orphan_claims = Vec::new();

    for path in walk_files(corpus_root)? {
        let relative = path
            .strip_prefix(corpus_root)
            .expect("walk_files only yields paths under corpus_root")
            .to_string_lossy()
            .replace(std::path::MAIN_SEPARATOR, "/");

        let Some(genre) = config.match_genre(&relative) else {
            continue;
        };

        let contents = std::fs::read_to_string(&path).map_err(|source| CorpusError::Read {
            path: relative.clone(),
            source,
        })?;

        let result = extract::extract_document(&relative, &contents);

        let stem = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| relative.clone());

        corpus.documents.push(Document {
            stem,
            file: relative,
            genre_path: genre.path.clone(),
            headings: result.headings,
        });
        corpus.claims.extend(result.claims);
        orphan_claims.extend(result.orphan_claims);
    }

    Ok(LoadedCorpus { corpus, orphan_claims })
}

/// A deterministic, hidden-file-skipping, symlink-skipping recursive file
/// walk. Hand-rolled rather than a `walkdir` dependency: the corpus trees
/// this tool targets are small, and the traversal rules are simple
/// (skip dotfiles/dotdirs — `.git`, `.ledger`, `.scratch` and friends —
/// don't follow symlinks to avoid cycles).
fn walk_files(root: &Path) -> Result<Vec<PathBuf>, CorpusError> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];

    while let Some(dir) = stack.pop() {
        let entries = std::fs::read_dir(&dir).map_err(|source| CorpusError::Walk {
            path: dir.display().to_string(),
            source,
        })?;
        for entry in entries {
            let entry = entry.map_err(|source| CorpusError::Walk {
                path: dir.display().to_string(),
                source,
            })?;
            let name = entry.file_name();
            if name.to_string_lossy().starts_with('.') {
                continue;
            }
            let file_type = entry.file_type().map_err(|source| CorpusError::Walk {
                path: entry.path().display().to_string(),
                source,
            })?;
            if file_type.is_dir() {
                stack.push(entry.path());
            } else if file_type.is_file() {
                out.push(entry.path());
            }
            // Symlinks (file_type.is_symlink()) are intentionally not
            // followed.
        }
    }

    out.sort();
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::load_config;
    use std::io::Write;

    struct TempDir(PathBuf);
    impl TempDir {
        fn path(&self) -> &Path {
            &self.0
        }
        fn write(&self, relative: &str, contents: &str) {
            let path = self.0.join(relative);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            let mut f = std::fs::File::create(path).unwrap();
            f.write_all(contents.as_bytes()).unwrap();
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn tempdir() -> TempDir {
        let dir = std::env::temp_dir().join(format!(
            "docket-corpus-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }

    #[test]
    fn scans_only_genre_matched_files() {
        let dir = tempdir();
        dir.write(
            "docket.ncl",
            r#"{ genres = [ { path = "docs/specs/**", kinds = ["constraint"] } ] }"#,
        );
        dir.write(
            "docs/specs/lock.md",
            "### [lock-groundness]\n\n```claim\nkind: constraint\n```\n",
        );
        dir.write("docs/other/ignored.md", "### [not-scanned]\n\n```claim\nkind: constraint\n```\n");
        dir.write("README.md", "# hello\n");

        let config = load_config(dir.path()).unwrap();
        let loaded = load_corpus(dir.path(), &config).unwrap();

        assert_eq!(loaded.corpus.documents.len(), 1);
        assert_eq!(loaded.corpus.documents[0].stem, "lock");
        assert_eq!(loaded.corpus.claims.len(), 1);
        assert_eq!(loaded.corpus.claims[0].id, "lock-groundness");
    }

    #[test]
    fn skips_dotfiles_and_dotdirs() {
        let dir = tempdir();
        dir.write(
            "docket.ncl",
            r#"{ genres = [ { path = "docs/**", kinds = ["constraint"] } ] }"#,
        );
        dir.write("docs/.hidden/should-not-be-seen.md", "### [x]\n\n```claim\nkind: constraint\n```\n");
        dir.write("docs/.dotfile.md", "### [y]\n\n```claim\nkind: constraint\n```\n");
        dir.write("docs/visible.md", "### [z]\n\n```claim\nkind: constraint\n```\n");

        let config = load_config(dir.path()).unwrap();
        let loaded = load_corpus(dir.path(), &config).unwrap();

        assert_eq!(loaded.corpus.claims.len(), 1);
        assert_eq!(loaded.corpus.claims[0].id, "z");
    }

    #[test]
    fn collects_orphan_claims_across_the_corpus() {
        let dir = tempdir();
        dir.write(
            "docket.ncl",
            r#"{ genres = [ { path = "docs/**", kinds = ["constraint"] } ] }"#,
        );
        dir.write("docs/orphan.md", "## Notes\n\n```claim\nkind: constraint\n```\n");

        let config = load_config(dir.path()).unwrap();
        let loaded = load_corpus(dir.path(), &config).unwrap();

        assert_eq!(loaded.corpus.claims.len(), 0);
        assert_eq!(loaded.orphan_claims.len(), 1);
        assert_eq!(loaded.orphan_claims[0].file, "docs/orphan.md");
    }
}
