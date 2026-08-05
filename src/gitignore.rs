//! `unreachable-reference`: a link from a tracked document to a
//! gitignored path (`.ledger/2026-08-05-references-that-leave-the-register.md`,
//! O4's "unreachable" condition — distinct from "dangles", the target
//! does not exist). Such a reference resolves perfectly for its author,
//! for a reviewer in the same worktree, and for every automated check
//! that runs where the author sits; it is unresolvable for every reader
//! of the repository, which is everyone the document was written for.
//!
//! **Directional by design**, per the head's ruling relayed in this
//! feature's dispatch: a tracked document pointing at an ignored path is
//! illegal; the reverse (an ignored file linking into the repository) is
//! fine, "because only the author has that." Nothing here ever inspects
//! a link's *source* for ignored-ness, only its target — and the
//! directionality is also structural, not merely a filter this module
//! applies: [`crate::corpus::load_corpus`] only ever walks tracked,
//! genre-matched files (dotdirs are skipped outright), so an ignored
//! file's own links never reach this module to begin with.
//!
//! Path resolution here is pure and mirrors `register.ncl`'s
//! `normalize_prose_link`/`resolve_relative`, duplicated rather than
//! shared for the same reason `checks.rs`'s own module doc names for
//! `CiteRef`/`anchor_matches`: this module's output (a corpus-relative
//! path to hand to `git`) is consumed by an I/O step Nickel cannot
//! perform, so the seam sits here rather than upstream of it. Determining
//! ignored-ness itself is impure — `git check-ignore` — and stays in
//! Rust per R6's own boundary: "Rust keeps what genuinely needs it: ...
//! asking git what is tracked."

use crate::extract::DocumentLink;
use crate::model::Line;
use std::collections::HashSet;
use std::io::Write as _;
use std::path::Path;
use std::process::{Command, Stdio};

/// A tracked document's link to a path `git` reports as ignored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IgnoredReference {
    pub file: String,
    pub line: Line,
    /// The link destination exactly as written — the offending value
    /// (MVP.md §3).
    pub dest: String,
    /// The corpus-relative path `dest` resolved to and what was actually
    /// checked against `.gitignore`.
    pub resolved: String,
}

/// Whether `dest`'s pre-anchor part is a real filesystem path rather than
/// a bare claim-id candidate — mirrors `register.ncl`'s
/// `normalize_prose_link`: a link with an anchor is *always* a document
/// reference there (a claim id carries no anchor of its own, `model.rs`'s
/// `CiteRef` grammar), and a link with no anchor is a path only if it
/// contains `/` or `.`. A claim id is lowercase-kebab by grammar
/// (`extract::is_kebab_case`) and can therefore never contain a `.`, so
/// that pair of signals is exact, not a heuristic guess: `[see
/// also](sibling-claim)` (no anchor, no slash, no dot) reads as a claim
/// citation and is out of scope here the same way it is for C5's own
/// path/claim-id split; `[see](notes.txt)` or `[see](../x)` reads as a
/// path and is in scope. Returns `None` for a same-file anchor-only link
/// too (`[see](#heading)`) — that always resolves to the citing file
/// itself, which is tracked by construction (it is the file being
/// scanned), so there is nothing this check could ever find there.
fn path_shaped(dest: &str) -> Option<&str> {
    let (path, has_anchor) = match dest.split_once('#') {
        Some((path, _)) => (path, true),
        None => (dest, false),
    };
    if path.is_empty() {
        None
    } else if has_anchor || path.contains('/') || path.contains('.') {
        Some(path)
    } else {
        None
    }
}

/// Lexically resolve `path` against the directory of `citing_file` —
/// mirrors `register.ncl`'s `resolve_relative`: a leading `/` is
/// corpus-root-relative, `..` pops a segment. `None` if a `..` would pop
/// past the corpus root (the same corpus-escape rule C5 already applies)
/// — this module has no more ability to say what's ignored in a
/// repository it can't see than C5 has to resolve a claim it can't see.
fn resolve(citing_file: &str, path: &str) -> Option<String> {
    let (base_dir, path) = match path.strip_prefix('/') {
        Some(rest) => ("", rest),
        None => (citing_file.rsplit_once('/').map_or("", |(d, _)| d), path),
    };

    let mut segments: Vec<&str> = if base_dir.is_empty() {
        Vec::new()
    } else {
        base_dir.split('/').collect()
    };

    for seg in path.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                segments.pop()?;
            }
            other => segments.push(other),
        }
    }
    Some(segments.join("/"))
}

/// Ask `git` which of `candidates` (corpus-root-relative paths) it would
/// ignore, batched into one `check-ignore --stdin` call rather than one
/// process per link. Degrades to "none ignored" — never a crash, never a
/// false positive — when `corpus_root` is not inside a git working tree
/// or `git` itself is not on `PATH`: a corpus with no git history to ask
/// has no reader-reachability question this check can answer, so silence
/// is the correct verdict, not an error. `git_bin` is `"git"` for every
/// real caller ([`find_unreachable_references`]); parameterized only so
/// the "binary not found" branch is directly testable (a nonexistent name
/// below) without depending on the test environment actually lacking
/// `git`.
fn ignored_paths(git_bin: &str, corpus_root: &Path, candidates: &[String]) -> HashSet<String> {
    if candidates.is_empty() {
        return HashSet::new();
    }

    let mut child = match Command::new(git_bin)
        .arg("-C")
        .arg(corpus_root)
        .arg("check-ignore")
        .arg("--stdin")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(child) => child,
        Err(_) => return HashSet::new(),
    };

    // Written from a separate thread rather than before `wait_with_output`:
    // `check-ignore --stdin` streams matches back as it reads, so a large
    // candidate list can fill its stdout pipe before this process has
    // finished writing stdin, deadlocking a strictly sequential
    // write-then-wait.
    let mut stdin = match child.stdin.take() {
        Some(stdin) => stdin,
        None => return HashSet::new(),
    };
    let input = candidates.join("\n");
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(input.as_bytes());
    });

    let output = match child.wait_with_output() {
        Ok(output) => output,
        Err(_) => return HashSet::new(),
    };
    let _ = writer.join();

    // Exit status: 0 = at least one candidate matched, 1 = none did (both
    // a legitimate result of a completed run), 128 = a real error — not a
    // git repository, an unreadable `corpus_root`, and so on. Only the
    // error case degrades to silence; "ran fine, nothing ignored" is
    // already an empty `stdout`.
    if output.status.code() == Some(128) {
        return HashSet::new();
    }

    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::to_string)
        .collect()
}

/// Every `unreachable-reference` finding across a corpus's whole link
/// surface: a path-shaped link in a tracked document that resolves to a
/// path `git` would not track. One batched `git` invocation for the
/// entire corpus, regardless of how many links it holds.
pub fn find_unreachable_references(
    corpus_root: &Path,
    links: &[DocumentLink],
) -> Vec<IgnoredReference> {
    let candidates: Vec<(&DocumentLink, String)> = links
        .iter()
        .filter_map(|link| {
            let path = path_shaped(&link.dest)?;
            let resolved = resolve(&link.file, path)?;
            Some((link, resolved))
        })
        .collect();

    let paths: Vec<String> = candidates.iter().map(|(_, r)| r.clone()).collect();
    let ignored = ignored_paths("git", corpus_root, &paths);

    candidates
        .into_iter()
        .filter(|(_, resolved)| ignored.contains(resolved))
        .map(|(link, resolved)| IgnoredReference {
            file: link.file.clone(),
            line: link.line,
            dest: link.dest.clone(),
            resolved,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- path_shaped -------------------------------------------------

    #[test]
    fn a_bare_word_with_no_slash_dot_or_anchor_is_a_claim_id_candidate() {
        assert_eq!(path_shaped("sibling-claim"), None);
    }

    #[test]
    fn a_same_file_anchor_only_link_is_not_path_shaped() {
        assert_eq!(path_shaped("#some-heading"), None);
    }

    #[test]
    fn an_empty_href_is_not_path_shaped() {
        assert_eq!(path_shaped(""), None);
    }

    #[test]
    fn a_slash_makes_a_bare_word_path_shaped() {
        assert_eq!(path_shaped("../notes"), Some("../notes"));
    }

    #[test]
    fn a_dot_makes_a_bare_word_path_shaped() {
        assert_eq!(path_shaped("notes.txt"), Some("notes.txt"));
    }

    #[test]
    fn an_anchor_makes_a_bare_word_path_shaped_even_with_no_slash_or_dot() {
        // register.ncl's own rule: presence of `#` always means a
        // document reference, never a bare claim id — a claim id carries
        // no anchor of its own.
        assert_eq!(path_shaped("sibling#3"), Some("sibling"));
    }

    #[test]
    fn an_anchor_with_an_empty_path_part_is_not_path_shaped() {
        assert_eq!(path_shaped("#3"), None);
    }

    // --- resolve -------------------------------------------------------

    #[test]
    fn resolves_relative_to_the_citing_files_directory() {
        assert_eq!(
            resolve("docs/specs/x.md", "../scratch/notes.md"),
            Some("docs/scratch/notes.md".to_string())
        );
    }

    #[test]
    fn resolves_a_leading_slash_as_corpus_root_relative() {
        assert_eq!(
            resolve("docs/specs/deep/x.md", "/.scratch/notes.md"),
            Some(".scratch/notes.md".to_string())
        );
    }

    #[test]
    fn a_sibling_file_resolves_within_the_same_directory() {
        assert_eq!(
            resolve("docs/specs/x.md", "sibling.md"),
            Some("docs/specs/sibling.md".to_string())
        );
    }

    #[test]
    fn escaping_the_corpus_root_resolves_to_none() {
        assert_eq!(resolve("x.md", "../../outside.md"), None);
    }

    // --- ignored_paths / find_unreachable_references (real git) --------

    struct TempRepo(std::path::PathBuf);
    impl TempRepo {
        fn write(&self, relative: &str, contents: &str) {
            let path = self.0.join(relative);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, contents).unwrap();
        }
    }
    impl Drop for TempRepo {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn git_repo(gitignore: &str) -> TempRepo {
        let dir = std::env::temp_dir().join(format!(
            "docket-gitignore-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let repo = TempRepo(dir);
        let status = Command::new("git")
            .arg("-C")
            .arg(&repo.0)
            .args(["init", "--quiet"])
            .status()
            .expect("git must be on PATH to run this test");
        assert!(status.success());
        repo.write(".gitignore", gitignore);
        repo
    }

    fn link(file: &str, dest: &str) -> DocumentLink {
        DocumentLink {
            file: file.to_string(),
            line: Line(1),
            dest: dest.to_string(),
        }
    }

    #[test]
    fn a_link_into_a_gitignored_directory_is_reported() {
        let repo = git_repo(".scratch/\n");
        repo.write("docs/specs/a.md", "");
        let links = vec![link("docs/specs/a.md", "../../.scratch/notes.md")];
        let found = find_unreachable_references(&repo.0, &links);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].file, "docs/specs/a.md");
        assert_eq!(found[0].resolved, ".scratch/notes.md");
        assert_eq!(found[0].dest, "../../.scratch/notes.md");
    }

    #[test]
    fn a_link_to_an_ordinary_tracked_path_is_silent() {
        let repo = git_repo(".scratch/\n");
        repo.write("docs/specs/a.md", "");
        repo.write("docs/specs/b.md", "");
        let links = vec![link("docs/specs/a.md", "b.md")];
        assert!(find_unreachable_references(&repo.0, &links).is_empty());
    }

    #[test]
    fn a_claim_id_shaped_bare_word_is_never_checked_even_if_it_would_match() {
        // "target" with no slash/dot/anchor is a claim-id candidate
        // (out of scope), even though a directory named `target` really
        // is gitignored here — proves the scope boundary is applied
        // before git is ever asked, not merely that this particular
        // corpus happens not to trigger it.
        let repo = git_repo("target/\n");
        repo.write("docs/specs/a.md", "");
        let links = vec![link("docs/specs/a.md", "target")];
        assert!(find_unreachable_references(&repo.0, &links).is_empty());
    }

    #[test]
    fn a_reference_escaping_the_corpus_root_is_not_checked() {
        let repo = git_repo(".scratch/\n");
        repo.write("docs/specs/a.md", "");
        let links = vec![link("docs/specs/a.md", "../../../outside.md")];
        assert!(find_unreachable_references(&repo.0, &links).is_empty());
    }

    #[test]
    fn a_corpus_root_that_is_not_a_git_repository_degrades_to_silence() {
        let dir = std::env::temp_dir().join(format!(
            "docket-gitignore-nogit-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let links = vec![link("docs/specs/a.md", "../../.scratch/notes.md")];
        let found = find_unreachable_references(&dir, &links);
        let _ = std::fs::remove_dir_all(&dir);
        assert!(
            found.is_empty(),
            "a non-repository corpus root must never crash or false-positive: {found:#?}"
        );
    }

    #[test]
    fn a_missing_git_binary_degrades_to_silence_rather_than_a_crash() {
        let dir = std::env::temp_dir();
        let result = ignored_paths(
            "docket-test-definitely-not-a-real-binary",
            &dir,
            &["anything".to_string()],
        );
        assert!(result.is_empty());
    }
}
