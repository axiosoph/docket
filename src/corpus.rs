//! Walk a corpus root, match each file against `docket.ncl`'s genres, and
//! extract claims from the ones that match. "Files matching no genre are
//! not scanned" (MVP.md §2).

use crate::config::{AmbiguousGenre, Config};
use crate::extract::{self, MalformedId, NormativeOccurrence, OrphanClaim, UnregisteredDefinition};
use crate::gitignore::{self, IgnoredReference};
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
    #[error(transparent)]
    AmbiguousGenre(#[from] AmbiguousGenre),
}

#[derive(Debug)]
pub struct LoadedCorpus {
    pub corpus: Corpus,
    pub orphan_claims: Vec<OrphanClaim>,
    /// Every RFC-2119 keyword found in a scanned document's own voice,
    /// genre-agnostic (extract.rs). Whether one is a violation depends on
    /// its document's genre, which only checks.rs's `normative-prose`
    /// check has in view — collected here the same way `orphan_claims` is.
    pub normative_occurrences: Vec<NormativeOccurrence>,
    /// Every recognized id definition (heading- or bold-form) with no
    /// claim block — the coverage count. Collected corpus-wide the same
    /// way `orphan_claims` is.
    pub unregistered_definitions: Vec<UnregisteredDefinition>,
    /// Every bracketed token in definition position whose inner content
    /// fails the id grammar — the `malformed-id` diagnostic. Collected
    /// corpus-wide the same way `orphan_claims` is.
    pub malformed_ids: Vec<MalformedId>,
    /// Every tracked-document link that resolves to a path `git` would
    /// not track — the `unreachable-reference` diagnostic
    /// (`.ledger/2026-08-05-references-that-leave-the-register.md`, O4).
    /// Computed here rather than in `checks.rs`/`register.ncl`: unlike
    /// every other field on this struct, deciding this one is impure (it
    /// asks `git`), so it belongs beside the rest of this module's I/O,
    /// not downstream of it.
    pub unreachable_references: Vec<IgnoredReference>,
}

/// Load every genre-matched file under `corpus_root`.
pub fn load_corpus(corpus_root: &Path, config: &Config) -> Result<LoadedCorpus, CorpusError> {
    let mut corpus = Corpus::default();
    let mut orphan_claims = Vec::new();
    let mut normative_occurrences = Vec::new();
    let mut unregistered_definitions = Vec::new();
    let mut malformed_ids = Vec::new();
    let mut links = Vec::new();

    for path in walk_files(corpus_root)? {
        let relative = path
            .strip_prefix(corpus_root)
            .expect("walk_files only yields paths under corpus_root")
            .to_string_lossy()
            .replace(std::path::MAIN_SEPARATOR, "/");

        let Some(genre) = config.match_genre(&relative)? else {
            continue;
        };

        // Only .md files are scanned (MVP.md §2): a claim block can only
        // live in markdown, so reading anything else is wasted work at
        // best. It's a real failure mode, not a hypothetical — a TLC
        // model-checker state dump (binary, no extension) sitting in a
        // generated subtree under a matched genre aborted the first run
        // against a real corpus. Extension-based, not content-sniffed:
        // sniffing would still have to open every file, the cost this
        // check exists to avoid.
        if path.extension().and_then(|ext| ext.to_str()) != Some("md") {
            continue;
        }

        let contents = std::fs::read_to_string(&path).map_err(|source| CorpusError::Read {
            path: relative.clone(),
            source,
        })?;

        let result = extract::extract_document(&relative, &contents);

        // The document identifier is the corpus-relative path with `.md`
        // removed (MVP.md §1.3) — unique by construction, unlike a bare
        // basename. `relative` is guaranteed to end in `.md` by the
        // extension filter above; `unwrap_or` is a defensive fallback,
        // not an expected path.
        let doc_path = relative
            .strip_suffix(".md")
            .unwrap_or(&relative)
            .to_string();

        corpus.documents.push(Document {
            doc_path,
            file: relative,
            genre_path: genre.path.clone(),
            headings: result.headings,
        });
        corpus.claims.extend(result.claims);
        orphan_claims.extend(result.orphan_claims);
        normative_occurrences.extend(result.normative_occurrences);
        unregistered_definitions.extend(result.unregistered_definitions);
        malformed_ids.extend(result.malformed_ids);
        links.extend(result.links);
    }

    // One batched `git check-ignore` for the whole corpus's link surface
    // — `gitignore::find_unreachable_references` is the only I/O this
    // function performs beyond reading files and `git`'s own filesystem
    // walk, and it needs every document's links gathered first.
    let unreachable_references = gitignore::find_unreachable_references(corpus_root, &links);

    Ok(LoadedCorpus {
        corpus,
        orphan_claims,
        normative_occurrences,
        unregistered_definitions,
        malformed_ids,
        unreachable_references,
    })
}

/// A deterministic, hidden-file-skipping, symlink-skipping recursive file
/// walk. Hand-rolled rather than a `walkdir` dependency: the corpus trees
/// this tool targets are small, and the traversal rules are simple
/// (skip dotfiles/dotdirs — `.git`, `.ledger`, `.scratch` and friends —
/// don't follow symlinks to avoid cycles).
///
/// `pub(crate)` rather than private: this walk is not specific to
/// documents. `marker.rs` reuses it unchanged to scan the *whole* corpus
/// tree for evaluator markers (source files, not just `.md` — the
/// extension filter below is this function's caller's decision, not
/// this function's).
pub(crate) fn walk_files(root: &Path) -> Result<Vec<PathBuf>, CorpusError> {
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
            self.write_bytes(relative, contents.as_bytes());
        }
        fn write_bytes(&self, relative: &str, contents: &[u8]) {
            let path = self.0.join(relative);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            let mut f = std::fs::File::create(path).unwrap();
            f.write_all(contents).unwrap();
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    impl TempDir {
        /// `git init` this directory so `gitignore::find_unreachable_references`
        /// has a real repository to ask `check-ignore` against — only the
        /// wiring test below needs this; every other fixture here is
        /// deliberately not a git repository at all, exercising the
        /// degrade-to-silence path implicitly.
        fn git_init(&self) {
            let status = std::process::Command::new("git")
                .arg("-C")
                .arg(&self.0)
                .args(["init", "--quiet"])
                .status()
                .expect("git must be on PATH to run this test");
            assert!(status.success());
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
            r#"{ genres = [ { path = "docs/specs/**", kinds = ["constraint"], quadrant = "reference" } ] }"#,
        );
        dir.write(
            "docs/specs/lock.md",
            "### [lock-groundness]\n\n```claim\nkind: constraint\n```\n",
        );
        dir.write(
            "docs/other/ignored.md",
            "### [not-scanned]\n\n```claim\nkind: constraint\n```\n",
        );
        dir.write("README.md", "# hello\n");

        let config = load_config(dir.path()).unwrap();
        let loaded = load_corpus(dir.path(), &config).unwrap();

        assert_eq!(loaded.corpus.documents.len(), 1);
        assert_eq!(loaded.corpus.documents[0].doc_path, "docs/specs/lock");
        assert_eq!(loaded.corpus.claims.len(), 1);
        assert_eq!(loaded.corpus.claims[0].id, "lock-groundness");
    }

    #[test]
    fn documents_sharing_a_basename_get_distinct_path_identifiers() {
        // The real-corpus shape that retired duplicate-stem: three
        // README.md files under one genre are legitimate, not a
        // collision, because the identifier is the whole path.
        let dir = tempdir();
        dir.write(
            "docket.ncl",
            r#"{ genres = [ { path = "docs/models/**", kinds = ["invariant"], quadrant = "reference" } ] }"#,
        );
        dir.write("docs/models/lean/README.md", "# Lean\n");
        dir.write("docs/models/tla/README.md", "# TLA\n");

        let config = load_config(dir.path()).unwrap();
        let loaded = load_corpus(dir.path(), &config).unwrap();

        let mut paths: Vec<&str> = loaded
            .corpus
            .documents
            .iter()
            .map(|d| d.doc_path.as_str())
            .collect();
        paths.sort();
        assert_eq!(
            paths,
            vec!["docs/models/lean/README", "docs/models/tla/README"]
        );
    }

    #[test]
    fn a_file_matching_two_genres_aborts_the_walk() {
        let dir = tempdir();
        dir.write(
            "docket.ncl",
            r#"{
              genres = [
                { path = "docs/**", kinds = ["requirement"], quadrant = "reference" },
                { path = "docs/specs/**", kinds = ["constraint"], quadrant = "reference" },
              ],
            }"#,
        );
        dir.write("docs/specs/x.md", "# hello\n");

        let config = load_config(dir.path()).unwrap();
        let err = load_corpus(dir.path(), &config).unwrap_err();
        assert!(matches!(err, CorpusError::AmbiguousGenre(_)));
    }

    #[test]
    fn skips_dotfiles_and_dotdirs() {
        let dir = tempdir();
        dir.write(
            "docket.ncl",
            r#"{ genres = [ { path = "docs/**", kinds = ["constraint"], quadrant = "reference" } ] }"#,
        );
        dir.write(
            "docs/.hidden/should-not-be-seen.md",
            "### [x]\n\n```claim\nkind: constraint\n```\n",
        );
        dir.write(
            "docs/.dotfile.md",
            "### [y]\n\n```claim\nkind: constraint\n```\n",
        );
        dir.write(
            "docs/visible.md",
            "### [z]\n\n```claim\nkind: constraint\n```\n",
        );

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
            r#"{ genres = [ { path = "docs/**", kinds = ["constraint"], quadrant = "reference" } ] }"#,
        );
        dir.write(
            "docs/orphan.md",
            "## Notes\n\n```claim\nkind: constraint\n```\n",
        );

        let config = load_config(dir.path()).unwrap();
        let loaded = load_corpus(dir.path(), &config).unwrap();

        assert_eq!(loaded.corpus.claims.len(), 0);
        assert_eq!(loaded.orphan_claims.len(), 1);
        assert_eq!(loaded.orphan_claims[0].file, "docs/orphan.md");
    }

    #[test]
    fn collects_malformed_ids_across_the_corpus() {
        // `.ledger/2026-08-04-malformed-ids-are-silently-invisible.md`:
        // the wiring layer between extract.rs and checks.rs, mirroring
        // `collects_orphan_claims_across_the_corpus` above for the new
        // field.
        let dir = tempdir();
        dir.write(
            "docket.ncl",
            r#"{ genres = [ { path = "docs/**", kinds = ["constraint"], quadrant = "reference" } ] }"#,
        );
        dir.write("docs/a.md", "**[boundary-L1-concerns]**: L1 owns things.\n");

        let config = load_config(dir.path()).unwrap();
        let loaded = load_corpus(dir.path(), &config).unwrap();

        assert_eq!(loaded.corpus.claims.len(), 0);
        assert_eq!(loaded.malformed_ids.len(), 1);
        assert_eq!(loaded.malformed_ids[0].file, "docs/a.md");
        assert_eq!(loaded.malformed_ids[0].id, "boundary-L1-concerns");
    }

    #[test]
    fn collects_unreachable_references_across_the_corpus() {
        // The wiring layer between extract.rs's document-level link scan
        // and gitignore.rs's git query, mirroring
        // `collects_malformed_ids_across_the_corpus` above for this field.
        let dir = tempdir();
        dir.git_init();
        dir.write(".gitignore", ".scratch/\n");
        dir.write(
            "docket.ncl",
            r#"{ genres = [ { path = "docs/**", kinds = ["constraint"], quadrant = "reference" } ] }"#,
        );
        dir.write(
            "docs/a.md",
            "### [x]\n\nSee [notes](../.scratch/notes.md).\n\n```claim\nkind: constraint\n```\n",
        );

        let config = load_config(dir.path()).unwrap();
        let loaded = load_corpus(dir.path(), &config).unwrap();

        assert_eq!(loaded.unreachable_references.len(), 1);
        assert_eq!(loaded.unreachable_references[0].file, "docs/a.md");
        assert_eq!(
            loaded.unreachable_references[0].resolved,
            ".scratch/notes.md"
        );
    }

    #[test]
    fn scans_cleanly_past_a_non_utf8_binary_file_in_a_matched_genre() {
        // The exact real-corpus failure mode (MVP.md §2): a TLC
        // model-checker state dump, binary and extensionless, sitting
        // inside docs/models/ where the genre pattern legitimately
        // matches it. The walk must skip it silently rather than abort.
        let dir = tempdir();
        dir.write(
            "docket.ncl",
            r#"{ genres = [ { path = "docs/models/**", kinds = ["invariant"], quadrant = "reference" } ] }"#,
        );
        dir.write(
            "docs/models/claim.md",
            "### [x]\n\n```claim\nkind: invariant\n```\n",
        );
        dir.write_bytes(
            "docs/models/tla/states/26-06-06-22-46-06/nodes_0",
            &[0xff, 0xfe, 0x00, 0x01, 0x80, 0x81, 0xc0, 0xc1],
        );
        // Also cover a non-markdown file that DOES decode as UTF-8, to
        // pin the rule as ".md specifically" rather than "whatever
        // doesn't crash the reader".
        dir.write("docs/models/notes.txt", "plain text, not markdown\n");

        let config = load_config(dir.path()).unwrap();
        let loaded = load_corpus(dir.path(), &config).expect("must not abort on the binary file");

        assert_eq!(loaded.corpus.claims.len(), 1);
        assert_eq!(loaded.corpus.claims[0].id, "x");
        assert_eq!(loaded.corpus.documents.len(), 1);
        assert_eq!(loaded.corpus.documents[0].file, "docs/models/claim.md");
    }
}
