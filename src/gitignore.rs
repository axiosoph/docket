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

/// One `check-ignore --stdin` invocation over `candidates`, returning the
/// exit code and whatever `stdout` held — `None` only if the process
/// itself never ran (spawn failure or the stdin handle wasn't there to
/// take). Split out of [`ignored_paths`] so the fallback path below can
/// reuse the exact same spawn/write/wait mechanics one candidate at a
/// time.
fn check_ignore_once(
    git_bin: &str,
    corpus_root: &Path,
    candidates: &[String],
) -> Option<(i32, HashSet<String>)> {
    let mut child = Command::new(git_bin)
        .arg("-C")
        .arg(corpus_root)
        .arg("check-ignore")
        .arg("--stdin")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;

    // Written from a separate thread rather than before `wait_with_output`:
    // `check-ignore --stdin` streams matches back as it reads, so a large
    // candidate list can fill its stdout pipe before this process has
    // finished writing stdin, deadlocking a strictly sequential
    // write-then-wait.
    let mut stdin = child.stdin.take()?;
    let input = candidates.join("\n");
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(input.as_bytes());
    });

    let output = child.wait_with_output().ok()?;
    let _ = writer.join();

    let found = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::to_string)
        .collect();
    Some((output.status.code().unwrap_or(-1), found))
}

/// Ask `git` which of `candidates` (corpus-root-relative paths) it would
/// ignore, batched into one `check-ignore --stdin` call rather than one
/// process per link. Degrades to "none ignored" — never a crash, never a
/// false positive — when `corpus_root` is not inside a git working tree
/// or `git` itself is not on `PATH`: a corpus with no git history to ask
/// has no reader-reachability question this check can answer, so silence
/// is the correct verdict, not an error. `git_bin` is `"git"` for every
/// real caller ([`find_unreachable_references`] and
/// [`crate::absence::find_literal`]); parameterized only so the "binary
/// not found" branch is directly testable (a nonexistent name below)
/// without depending on the test environment actually lacking `git`.
///
/// **Exit code 128 falls back to one candidate at a time**, rather than
/// degrading straight to empty the way the "not a repository"/"binary
/// missing" cases do. `git check-ignore --stdin` doesn't skip a
/// candidate it treats as an invalid pathspec (`/`, `..`, a leading
/// `//`, and surely others this scan hasn't hit yet) the way it skips an
/// ordinary non-ignored path — it FATALS the whole stream at 128 and
/// stops reading further candidates entirely, discarding every real
/// match queued after the bad one along with it. Confirmed directly
/// against a real corpus, not a hypothetical: this project's own prose
/// citing `/docs/...` and `// not a comment` in inline code spans each
/// independently zeroed out an otherwise-correct batch before this
/// fallback existed. 128 is ALSO the genuine "not a repository" signal,
/// which the fallback handles for free — every per-candidate retry hits
/// the identical 128 in that case, so the result still degrades to
/// empty, just through N+1 invocations instead of one.
///
/// `pub(crate)` rather than private: `absence::find_literal` reuses this
/// unchanged to keep build output and vendored dependencies (`target/`,
/// `node_modules/`, …) out of the search corpus, for the same reason
/// [`find_unreachable_references`] needs it here.
pub(crate) fn ignored_paths(
    git_bin: &str,
    corpus_root: &Path,
    candidates: &[String],
) -> HashSet<String> {
    if candidates.is_empty() {
        return HashSet::new();
    }

    let Some((code, found)) = check_ignore_once(git_bin, corpus_root, candidates) else {
        return HashSet::new();
    };

    if code != 128 {
        return found;
    }

    candidates
        .iter()
        .filter(|c| {
            matches!(
                check_ignore_once(git_bin, corpus_root, std::slice::from_ref(c)),
                Some((0, _))
            )
        })
        .cloned()
        .collect()
}

/// Every `unreachable-reference` finding across a corpus's whole
/// reference surface: `links` (markdown hrefs, resolved relative to the
/// citing file's directory, exactly as before) **and** `code_references`
/// (backtick-delimited path-shaped spans — inline code spans in markdown,
/// or a lexical backtick scan over a non-markdown source file, see
/// [`find_backtick_references`] — resolved as **corpus-root-relative
/// directly**, not against the citing file's directory).
///
/// The two resolution rules differ deliberately, not by oversight: a
/// markdown href is written the way a relative link genuinely works
/// (`../models/x.md`), but a code-span citation like `` `.ledger/foo.md` ``
/// carries no such prefix regardless of which file cites it — real
/// instances in this project's own corpus write it identically whether
/// the citing file sits at the corpus root or three directories deep
/// (`contracts/register.ncl`), which only makes sense if the author
/// already means "the corpus's `.ledger/`," not "relative to me."
/// Resolving it the same way a markdown href resolves would silently
/// point every non-root citer at the wrong (nonexistent, and therefore
/// falsely *not* unreachable) path.
///
/// One batched `git` invocation for the entire corpus, regardless of how
/// many links or code references it holds.
/// Whether `p` is too degenerate to ever hand `git check-ignore` — `/`,
/// `..`, or empty. `git check-ignore --stdin` doesn't just decline these
/// (as it does for an ordinary non-ignored path); it exits **128**,
/// "fatal: outside repository" — the SAME exit code this module already
/// treats as "not a real git repository, degrade to silence." One
/// degenerate candidate in a batched call therefore poisons the *entire*
/// batch: every real finding alongside it silently vanishes too, which
/// is a strictly worse failure than "this one candidate was skipped."
/// Confirmed directly against real `git` output (`/`, `..`, and `""` all
/// fatal at 128; `.` alone does not) rather than assumed. `"a/b"`-shaped
/// bare slashes never arise this way — `resolve` already rejects a `..`
/// that escapes the corpus root — so this exists specifically for
/// `code_references`, which (deliberately, see this function's own doc
/// comment) skip `resolve` entirely: a real corpus's own prose
/// discussing the leading-`/` convention in an inline code span
/// (`` `/` ``) is exactly the shape that surfaced this.
fn too_degenerate_for_git(p: &str) -> bool {
    p.is_empty() || p == "/" || p == ".."
}

pub fn find_unreachable_references(
    corpus_root: &Path,
    links: &[DocumentLink],
    code_references: &[DocumentLink],
) -> Vec<IgnoredReference> {
    // `unreachable-reference` means "resolves, but a reader can't follow
    // it" — this module's own doc comment already draws that line against
    // `dangling-reference` ("distinct from... the target does not exist",
    // top of file). The two candidate branches below discriminate "this is
    // a real citation" from "this merely looks like one" DIFFERENTLY, and
    // deliberately — the same `Path::exists` test would answer the
    // question wrong on one branch and right on the other.
    //
    // For a markdown link, existence is the right test: explicit link
    // syntax an author wrote to be followed either names something real
    // (this check's business) or names nothing at all — a typo or a
    // dead link, which is `dangling-reference`'s question, pinned by
    // `a_markdown_link_matching_a_gitignore_pattern_but_naming_nothing_
    // that_exists_is_dangling_not_unreachable` in checks.rs. That test
    // must keep passing unchanged; this filter is why it does.
    //
    // For a code span, existence answers a DIFFERENT question than the one
    // this check needs — and the wrong one, because it is a fact about the
    // *reader running the check*, not about the citation itself: a
    // gitignored path that genuinely exists on the author's own disk
    // (`.ledger/2026-08-05-notes.md`, once written) exists there and
    // nowhere else, so testing existence measures whichever checkout is
    // asking, not whether the reference is reachable. Confirmed against a
    // real corpus (`docket check` on axios): 120 findings in the author's
    // checkout, 118 in a clean worktree at the identical commit — the two
    // dropped were exactly this, a citation to a real local note that a
    // fresh clone or CI can never see. `unreachable-reference` exists
    // *for* the fresh-clone/CI reader (this module's own top-of-file doc:
    // "unresolvable for every reader ... which is everyone the document
    // was written for"); gating on the author's own disk state silences
    // the check for the one reader it does not need protecting from and
    // stays silent for every reader it does. The `.md` extension filter
    // below already does the discrimination existence was standing in
    // for — see its own comment — so this branch drops existence entirely.
    let exists_on_disk = |resolved: &str| corpus_root.join(resolved).exists();

    let mut candidates: Vec<(&DocumentLink, String)> = links
        .iter()
        .filter_map(|link| {
            let path = path_shaped(&link.dest)?;
            let resolved = resolve(&link.file, path)?;
            (!too_degenerate_for_git(&resolved) && exists_on_disk(&resolved))
                .then_some((link, resolved))
        })
        .collect();
    candidates.extend(code_references.iter().filter_map(|r| {
        let path = path_shaped(&r.dest)?;
        // A leading `/` means the same thing here it does for a markdown
        // href (§1.3: corpus-root-relative) — but unlike a markdown href,
        // this candidate is handed to `git` almost verbatim, and `git
        // check-ignore` reads a leading `/` as an OS-absolute path
        // attempt, not a repo-relative one (`fatal: Invalid path
        // '/docs': No such file or directory`, confirmed directly — a
        // real corpus's own prose discussing that exact convention,
        // `` `/docs/...` ``, is what surfaced this). Strip it before
        // resolving, same meaning either way.
        let resolved = path.strip_prefix('/').unwrap_or(path).to_string();
        // Document-shaped only, `.md` exactly — deliberately NOT gated on
        // existence (see this function's opening comment for why: on this
        // branch existence measures the checkout asking, not the
        // citation). Extension alone tells a citation apart from a prose
        // example that happens to name a real gitignored ARTIFACT:
        // `` `build/out.txt` `` in running text genuinely exists once an
        // author has run a build locally, same as
        // `.ledger/2026-08-05-foo.md` genuinely exists once an author has
        // written a note — existence cannot distinguish them, but every
        // real citation this check exists for is `.ledger/…md` (this
        // module's own top-of-file doc example), so `.md` does. Markdown-
        // link candidates are deliberately NOT filtered this way: a
        // markdown href is explicit link syntax an author wrote to be
        // followed, not incidental prose a backtick span merely happens
        // to look like — the same asymmetry `path_shaped`'s own doc
        // comment already draws between an authored link and a bare
        // mention.
        (!too_degenerate_for_git(&resolved) && resolved.ends_with(".md")).then_some((r, resolved))
    }));

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

/// Backtick-delimited spans in a non-markdown source file's raw text,
/// treated as `unreachable-reference` candidates the same way a markdown
/// inline code span is (`extract::extract_document`'s `code_references`)
/// — this project's own Nickel comments already write path citations in
/// that convention (`` `.ledger/…md` ``) even though `.ncl` has no
/// comparable parser here to lean on.
///
/// **A lexical, line-by-line scan, deliberately not comment-aware** — the
/// residual this carries, stated rather than hidden: it does not
/// distinguish a backtick pair inside a `#` comment from one inside a
/// string literal, and it does not follow a span across a newline (every
/// real citation in this project's `.ncl` files is single-line). Nickel
/// has no backtick syntax of its own — strings are `"…"` or `m%"…"%m` —
/// so in practice every backtick pair this scan finds in a well-formed
/// `.ncl` file sits inside a comment; a file that put a literal backtick
/// inside a string would be invisible to (or misread by) this scan, the
/// same class of stated limitation `absence.rs` already carries for
/// single-quoted strings and raw strings.
pub fn find_backtick_references(file: &str, text: &str) -> Vec<DocumentLink> {
    let mut out = Vec::new();
    for (idx, line) in text.lines().enumerate() {
        let mut rest = line;
        while let Some(open) = rest.find('`') {
            let after_open = &rest[open + 1..];
            let Some(close) = after_open.find('`') else {
                break;
            };
            let inner = &after_open[..close];
            out.push(DocumentLink {
                file: file.to_string(),
                line: Line(idx + 1),
                dest: inner.to_string(),
            });
            rest = &after_open[close + 1..];
        }
    }
    out
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
        repo.write(".scratch/notes.md", "");
        let links = vec![link("docs/specs/a.md", "../../.scratch/notes.md")];
        let found = find_unreachable_references(&repo.0, &links, &[]);
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
        assert!(find_unreachable_references(&repo.0, &links, &[]).is_empty());
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
        assert!(find_unreachable_references(&repo.0, &links, &[]).is_empty());
    }

    #[test]
    fn a_reference_escaping_the_corpus_root_is_not_checked() {
        let repo = git_repo(".scratch/\n");
        repo.write("docs/specs/a.md", "");
        let links = vec![link("docs/specs/a.md", "../../../outside.md")];
        assert!(find_unreachable_references(&repo.0, &links, &[]).is_empty());
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
        let found = find_unreachable_references(&dir, &links, &[]);
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

    #[test]
    fn a_fatal_candidate_falls_back_to_recovering_every_other_one() {
        // The batch-poisoning defect this fallback exists for, isolated
        // at the `ignored_paths` layer directly rather than only
        // end-to-end: `git check-ignore --stdin` fatals (128) on a
        // candidate it treats as an invalid pathspec — proven here with
        // a real repo and a real fatal candidate (`/`), not a mock — and
        // stops reading the stream at that point. Real matches queued on
        // BOTH sides of the bad one must still come back.
        let repo = git_repo(".ledger/\n");
        let candidates = vec![
            ".ledger/before.md".to_string(),
            "/".to_string(),
            ".ledger/after.md".to_string(),
            "not-ignored.md".to_string(),
        ];
        let found = ignored_paths("git", &repo.0, &candidates);
        assert_eq!(
            found,
            HashSet::from([
                ".ledger/before.md".to_string(),
                ".ledger/after.md".to_string(),
            ]),
            "a fatal candidate must not erase the real matches around it"
        );
    }

    #[test]
    fn a_fatal_candidate_alone_still_degrades_to_empty() {
        let repo = git_repo(".ledger/\n");
        let found = ignored_paths("git", &repo.0, &["/".to_string()]);
        assert!(found.is_empty());
    }

    // --- find_backtick_references ---------------------------------------

    #[test]
    fn a_single_backtick_span_is_extracted_with_its_line_number() {
        let text = "line one\nsee `.ledger/notes.md` for background\n";
        let found = find_backtick_references("contracts/x.ncl", text);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].file, "contracts/x.ncl");
        assert_eq!(found[0].line, Line(2));
        assert_eq!(found[0].dest, ".ledger/notes.md");
    }

    #[test]
    fn multiple_backtick_spans_on_one_line_are_all_extracted() {
        let text = "# see `a.ncl` and `b.ncl` both\n";
        let found = find_backtick_references("x.ncl", text);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].dest, "a.ncl");
        assert_eq!(found[1].dest, "b.ncl");
    }

    #[test]
    fn an_unclosed_backtick_on_a_line_yields_nothing_for_that_line() {
        let text = "this has one stray ` backtick only\n";
        assert!(find_backtick_references("x.ncl", text).is_empty());
    }

    #[test]
    fn a_span_never_crosses_a_newline() {
        // Every real citation in this project's own .ncl files is
        // single-line; two backticks on separate lines are two unclosed
        // spans, not one that happens to span a line break.
        let text = "opens here `\ncloses here `\n";
        assert!(find_backtick_references("x.ncl", text).is_empty());
    }

    #[test]
    fn a_backtick_span_with_no_path_shaped_content_is_still_captured() {
        // Filtering by `path_shaped` is `find_unreachable_references`'s
        // job, not this scanner's — an ordinary `` `cargo test` `` span
        // is captured here and filtered downstream.
        let text = "run `cargo test` first\n";
        let found = find_backtick_references("x.ncl", text);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].dest, "cargo test");
    }

    // --- find_unreachable_references, the code_references parameter -----

    #[test]
    fn a_code_reference_to_a_gitignored_path_is_reported() {
        let repo = git_repo(".ledger/\n");
        repo.write("contracts/x.ncl", "");
        repo.write(".ledger/2026-01-01-notes.md", "");
        let code_refs = vec![link("contracts/x.ncl", ".ledger/2026-01-01-notes.md")];
        let found = find_unreachable_references(&repo.0, &[], &code_refs);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].file, "contracts/x.ncl");
        assert_eq!(found[0].resolved, ".ledger/2026-01-01-notes.md");
    }

    #[test]
    fn a_code_reference_resolves_corpus_root_relative_not_citing_file_relative() {
        // The deliberate divergence from markdown-link resolution: a
        // code reference written inside `contracts/x.ncl` still means
        // "the corpus's own .ledger/", not "contracts/.ledger/" (which
        // resolving against the citing file's directory, the way a
        // markdown href does, would incorrectly produce).
        // Anchored (`/.ledger/`) so only the root-level directory is
        // ignored — `contracts/.ledger/` deliberately is NOT, so the test
        // can tell "resolved to the right (ignored) path" apart from
        // "resolved to the wrong (not ignored) one" instead of both
        // happening to match the same unanchored pattern.
        let repo = git_repo("/.ledger/\n");
        repo.write("contracts/x.ncl", "");
        repo.write("contracts/.ledger/notes.md", "");
        repo.write(".ledger/notes.md", "");
        let code_refs = vec![link("contracts/x.ncl", ".ledger/notes.md")];
        let found = find_unreachable_references(&repo.0, &[], &code_refs);
        assert_eq!(found.len(), 1);
        assert_eq!(
            found[0].resolved, ".ledger/notes.md",
            "must resolve to the corpus root's .ledger/, not contracts/.ledger/"
        );
    }

    #[test]
    fn a_non_path_shaped_code_reference_never_fires() {
        let repo = git_repo(".ledger/\n");
        repo.write("contracts/x.ncl", "");
        let code_refs = vec![link("contracts/x.ncl", "cargo test")];
        assert!(find_unreachable_references(&repo.0, &[], &code_refs).is_empty());
    }

    #[test]
    fn a_code_reference_to_an_ordinary_tracked_path_is_silent() {
        let repo = git_repo(".ledger/\n");
        repo.write("contracts/x.ncl", "");
        repo.write("contracts/y.ncl", "");
        let code_refs = vec![link("contracts/x.ncl", "contracts/y.ncl")];
        assert!(find_unreachable_references(&repo.0, &[], &code_refs).is_empty());
    }

    // --- code_references: document-shaped only (`.md`) --------------------
    //
    // Existence alone cannot tell a citation from a prose example naming a
    // real gitignored ARTIFACT: `build/out.txt` genuinely exists once an
    // author has run a build, the same way `.ledger/foo.md` genuinely
    // exists once an author has written a note. The code-span route
    // additionally requires `.md`, since every real citation this check
    // exists for is `.ledger/…md` — never applied to the `links` route,
    // which stays governed by existence alone (an authored markdown href
    // is not incidental prose).

    #[test]
    fn a_code_reference_to_a_real_gitignored_non_markdown_path_never_fires() {
        let repo = git_repo("build/\n");
        repo.write("contracts/x.ncl", "");
        repo.write("build/out.txt", "");
        let code_refs = vec![link("contracts/x.ncl", "build/out.txt")];
        assert!(
            find_unreachable_references(&repo.0, &[], &code_refs).is_empty(),
            "a real, gitignored, non-.md code-span candidate must never fire"
        );
    }

    #[test]
    fn a_code_reference_to_a_real_gitignored_markdown_path_still_fires() {
        let repo = git_repo("build/\n");
        repo.write("contracts/x.ncl", "");
        repo.write("build/notes.md", "");
        let code_refs = vec![link("contracts/x.ncl", "build/notes.md")];
        let found = find_unreachable_references(&repo.0, &[], &code_refs);
        assert_eq!(found.len(), 1, "{found:#?}");
        assert_eq!(found[0].resolved, "build/notes.md");
    }

    #[test]
    fn a_link_to_a_real_gitignored_non_markdown_path_still_fires() {
        // The asymmetry, pinned directly: the SAME non-.md target, gitignored
        // and genuinely existing, still fires through the `links` route —
        // the `.md` restriction is code-span-only.
        let repo = git_repo("build/\n");
        repo.write("docs/a.md", "");
        repo.write("build/out.txt", "");
        let links = vec![link("docs/a.md", "/build/out.txt")];
        let found = find_unreachable_references(&repo.0, &links, &[]);
        assert_eq!(found.len(), 1, "{found:#?}");
    }

    // --- too_degenerate_for_git ------------------------------------------
    //
    // The real defect this migration surfaced: `git check-ignore --stdin`
    // doesn't skip `/`, `..`, or an empty candidate the way it does an
    // ordinary non-ignored path — it FATALS (exit 128), which this
    // module already treats as "not a real repository, degrade silently."
    // Confirmed directly against real `git`, not assumed. Left
    // unfiltered, ONE such candidate in a batch would zero out every
    // real finding alongside it — a real corpus's own prose (MVP.md
    // §1.3, discussing the leading-`/` convention in an inline code
    // span) hit exactly this.

    #[test]
    fn a_bare_slash_code_reference_never_poisons_the_whole_batch() {
        let repo = git_repo(".ledger/\n");
        repo.write("MVP.md", "");
        repo.write(".ledger/notes.md", "");
        let code_refs = vec![link("MVP.md", "/"), link("MVP.md", ".ledger/notes.md")];
        let found = find_unreachable_references(&repo.0, &[], &code_refs);
        assert_eq!(
            found.len(),
            1,
            "the degenerate `/` candidate must not silence the real finding beside it: {found:#?}"
        );
        assert_eq!(found[0].resolved, ".ledger/notes.md");
    }

    #[test]
    fn a_dotdot_code_reference_never_poisons_the_whole_batch() {
        let repo = git_repo(".ledger/\n");
        repo.write("MVP.md", "");
        repo.write(".ledger/notes.md", "");
        let code_refs = vec![link("MVP.md", ".."), link("MVP.md", ".ledger/notes.md")];
        let found = find_unreachable_references(&repo.0, &[], &code_refs);
        assert_eq!(found.len(), 1, "{found:#?}");
        assert_eq!(found[0].resolved, ".ledger/notes.md");
    }

    #[test]
    fn a_bare_slash_link_never_poisons_the_whole_batch() {
        // The same guard also protects the `links` path, even though a
        // markdown href's own resolution rarely produces "/" — cheap
        // insurance against the identical git behaviour on that side too.
        let repo = git_repo(".ledger/\n");
        repo.write("docs/a.md", "");
        repo.write(".ledger/notes.md", "");
        let links = vec![
            link("docs/a.md", "/"),
            link("docs/a.md", "../.ledger/notes.md"),
        ];
        let found = find_unreachable_references(&repo.0, &links, &[]);
        assert_eq!(found.len(), 1, "{found:#?}");
    }
}
