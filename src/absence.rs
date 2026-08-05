//! Absence claims: `evaluator: absent` (`contracts/claim.ncl`) is
//! discharged by confirming a **named literal does not occur** in the
//! corpus's non-documentation source, rather than by any evidence that
//! something holds — see `run.rs`'s module docs, "Absence claims," for
//! the full design (the marker form chosen, why a failure is `Outcome::
//! Fail` like any other, and the fragment-tiling pattern for a literal
//! assembled across files).
//!
//! This module is the mechanism `run.rs` calls into: which files count
//! as "source" to search ([`find_literal`]), and the `absent-marker-
//! stale` diagnostic ([`find_stale_markers`]) that catches a marker's
//! literal drifting out of step with the prose that named it
//! (`.ledger/2026-08-05-references-that-leave-the-register.md`, O3:
//! "mark the literal, not a region of prose... a stale marker... is
//! REPORTED and does not fail").

use crate::corpus::{self, CorpusError};
use crate::marker::Marker;
use crate::model::{Claim, ClaimId, Corpus, Line};
use std::path::Path;

/// One place a claimed-absent literal was found in searched source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceHit {
    pub file: String,
    pub line: Line,
}

/// A recognized line/block comment syntax for one file extension — a
/// small table, not a lexer, the same shape choice `run.rs`'s
/// `VACUITY_SIGNALS` already makes and for the same reason: adding a
/// language is adding a row, never touching the matching logic. `line:
/// None, block: None` (an unrecognized extension) means the file is
/// searched **unstripped** — the conservative direction for this
/// feature: an unstripped comment can produce a false `Fail` (a mention
/// inside a comment reads as the literal having reappeared), which is a
/// nuisance an author investigates and fixes at the marker; a
/// wrongly-stripped real occurrence would instead produce a false
/// `Pass` — silent drift, exactly what this feature exists to catch.
/// Between the two failure directions, over-reporting is the one this
/// tool is allowed to make.
struct CommentSyntax {
    line: Option<&'static str>,
    block: Option<(&'static str, &'static str)>,
}

fn comment_syntax(extension: &str) -> CommentSyntax {
    match extension {
        "rs" | "go" | "java" | "js" | "jsx" | "ts" | "tsx" | "c" | "h" | "cc" | "cpp" | "hpp"
        | "swift" | "kt" | "scala" | "rescript" => CommentSyntax {
            line: Some("//"),
            block: Some(("/*", "*/")),
        },
        "py" | "sh" | "bash" | "zsh" | "rb" | "toml" | "yaml" | "yml" | "ncl" | "pl" => {
            CommentSyntax {
                line: Some("#"),
                block: None,
            }
        }
        "nix" => CommentSyntax {
            line: Some("#"),
            block: Some(("/*", "*/")),
        },
        "tla" => CommentSyntax {
            line: Some("\\*"),
            block: Some(("(*", "*)")),
        },
        "lean" => CommentSyntax {
            line: Some("--"),
            block: Some(("/-", "-/")),
        },
        "sql" => CommentSyntax {
            line: Some("--"),
            block: Some(("/*", "*/")),
        },
        "hs" => CommentSyntax {
            line: Some("--"),
            block: Some(("{-", "-}")),
        },
        "lua" => CommentSyntax {
            line: Some("--"),
            block: Some(("--[[", "]]")),
        },
        "als" => CommentSyntax {
            line: Some("--"),
            block: Some(("/*", "*/")),
        },
        "adb" | "ads" => CommentSyntax {
            line: Some("--"),
            block: None,
        },
        _ => CommentSyntax {
            line: None,
            block: None,
        },
    }
}

/// Append `s` to `out` with every character replaced by a single ASCII
/// space, except newlines (kept verbatim so line numbers never shift).
/// Always steps a whole `char` at a time, so the result is valid UTF-8
/// regardless of what multi-byte codepoints `s` held — never splits one
/// across a byte boundary.
fn blank_into(out: &mut String, s: &str) {
    for ch in s.chars() {
        out.push(if ch == '\n' { '\n' } else { ' ' });
    }
}

/// If `rest` opens with a double-quoted string literal (`"`), the byte
/// length of that whole literal — opening quote through the first
/// unescaped closing quote, or through the end of `rest` if it is never
/// closed. `None` if `rest` does not open with `"` at all. Shared by
/// [`strip_comments`] and [`brace_span`]/[`strip_rust_test_items`] (one
/// state machine, not two): a comment leader, a comment opener, and a
/// brace sitting inside a string literal must never be mistaken for a
/// real one — the merge gate reproduced all three as false `Pass`.
///
/// Escape-aware (`\"` does not close the string) but **not** aware of
/// every quoting convention every language `comment_syntax` lists uses:
/// it recognizes only `"..."`, never a single-quoted string (SQL, Lua)
/// or char literal (Rust, C, Haskell), because `'` is ALSO Rust's
/// lifetime sigil (`'a`) and there is no per-extension dispatch here to
/// tell a lifetime from an unterminated char literal safely; and it is
/// not raw-string-aware (Rust's `r"..."`/`r#"..."#` do not treat `\` as
/// an escape, so a raw string ending in a literal backslash right
/// before its closing quote can be misjudged). Both residuals are
/// documented in MVP.md rather than silently accepted.
fn skip_string_literal(rest: &str) -> Option<usize> {
    if !rest.starts_with('"') {
        return None;
    }
    let mut idx = 1; // the opening `"` itself
    while idx < rest.len() {
        let ch = rest[idx..].chars().next().expect("idx < rest.len()");
        match ch {
            '\\' => {
                idx += ch.len_utf8();
                if let Some(escaped) = rest[idx..].chars().next() {
                    idx += escaped.len_utf8();
                }
            }
            '"' => return Some(idx + ch.len_utf8()),
            _ => idx += ch.len_utf8(),
        }
    }
    Some(idx) // unterminated: the "string" runs to the end of `rest`
}

/// Blank every comment in `contents` to spaces, line count and every
/// newline preserved exactly — so a hit's line number, computed
/// afterward by simple line indexing, still points at the original
/// file. Heuristic, not a real lexer for any of the languages `syntax`
/// covers — a comment leader or opener is recognized only *outside* a
/// double-quoted string literal ([`skip_string_literal`]'s scope note
/// states exactly which quoting shapes that does and does not cover).
fn strip_comments(contents: &str, syntax: &CommentSyntax) -> String {
    let mut out = String::with_capacity(contents.len());
    let mut rest = contents;
    let mut in_block = false;

    while !rest.is_empty() {
        if in_block {
            if let Some((_, close)) = syntax.block
                && let Some(idx) = rest.find(close)
            {
                blank_into(&mut out, &rest[..idx]);
                blank_into(&mut out, close);
                rest = &rest[idx + close.len()..];
                in_block = false;
                continue;
            }
            blank_into(&mut out, rest);
            break;
        }

        if let Some(len) = skip_string_literal(rest) {
            // Copied through verbatim, never blanked: the string's
            // content may genuinely hold the searched-for literal, and
            // nothing inside it can open a real comment.
            out.push_str(&rest[..len]);
            rest = &rest[len..];
            continue;
        }

        if let Some((open, _)) = syntax.block
            && rest.starts_with(open)
        {
            blank_into(&mut out, open);
            rest = &rest[open.len()..];
            in_block = true;
            continue;
        }
        if let Some(leader) = syntax.line
            && rest.starts_with(leader)
        {
            let nl = rest.find('\n').unwrap_or(rest.len());
            blank_into(&mut out, &rest[..nl]);
            rest = &rest[nl..];
            continue;
        }

        // No comment construct opens here: copy exactly one character
        // (never a raw byte) through unchanged, so every subsequent
        // slice above stays on a valid UTF-8 boundary.
        let ch = rest.chars().next().expect("rest is non-empty");
        out.push(ch);
        rest = &rest[ch.len_utf8()..];
    }
    out
}

/// From the byte offset of an attribute marker (`#[cfg(test)]` or
/// `#[test]`) to one past the `}` that closes the first `{...}` group
/// after it — ASCII brace-counted, not a real parser, but a brace
/// inside a double-quoted string literal is skipped via
/// [`skip_string_literal`] (the same primitive [`strip_comments`] uses)
/// rather than counted. `None` if no `{` appears, or braces never
/// balance, before end of input.
fn brace_span(s: &str) -> Option<usize> {
    let open = s.find('{')?;
    let mut depth = 0i32;
    let mut idx = open;
    while idx < s.len() {
        if let Some(len) = skip_string_literal(&s[idx..]) {
            idx += len;
            continue;
        }
        let ch = s[idx..].chars().next().expect("idx < s.len()");
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(idx + ch.len_utf8());
                }
            }
            _ => {}
        }
        idx += ch.len_utf8();
    }
    None
}

/// Rust-specific: blank the whole span of every `#[cfg(test)]`- or
/// `#[test]`-attributed item — a `mod tests { ... }` block or a single
/// `#[test]` function, the "test-only items" O5 names as the second
/// half of the stripping rule ("source must be searched with test
/// paths, comments and test-only items excluded... one literal drew 29
/// raw matches across the tree and exactly one survived stripping").
/// Only ever applied to `.rs` files, and only after [`strip_comments`]
/// has already run, so a brace inside a comment can never be mistaken
/// for the item's own body.
fn strip_rust_test_items(contents: &str) -> String {
    const MARKERS: [&str; 2] = ["#[cfg(test)]", "#[test]"];
    let mut out = String::with_capacity(contents.len());
    let mut rest = contents;
    loop {
        let hit = MARKERS
            .iter()
            .filter_map(|m| rest.find(m).map(|i| (i, m.len())))
            .min_by_key(|(i, _)| *i);
        let Some((start, _)) = hit else {
            out.push_str(rest);
            break;
        };
        out.push_str(&rest[..start]);
        let after_marker = &rest[start..];
        match brace_span(after_marker) {
            Some(end) => {
                blank_into(&mut out, &after_marker[..end]);
                rest = &after_marker[end..];
            }
            None => {
                // No balanced brace found (end of file right after the
                // attribute, or a malformed file) — blank only the
                // attribute's own line and continue, rather than
                // silently consuming everything after it.
                let nl = after_marker
                    .find('\n')
                    .map(|i| i + 1)
                    .unwrap_or(after_marker.len());
                blank_into(&mut out, &after_marker[..nl]);
                rest = &after_marker[nl..];
            }
        }
    }
    out
}

/// Whether `relative` (a corpus-relative path, `/`-separated) has a
/// path component that names a test directory — `tests/` (Rust
/// integration tests) or `test/` (the common convention elsewhere),
/// exact and case-insensitive. A deliberately narrow rule: it is a
/// judgment call, not a rule this project's evidence pins the way it
/// pins comment/test-item stripping — a broader net (`spec/`,
/// `__tests__/`, a `*_test.*` filename pattern) was considered and
/// left out rather than guessed, since over-excluding a path silently
/// produces the same false-`Pass` risk `strip_comments` is built to
/// avoid. Narrower is the safer direction to start from; broadening it
/// is a small, targeted change if a real corpus needs it.
///
/// **The residual this narrowing does not close**: an exact `test`/
/// `tests` segment excludes a path even when it names real, always-
/// compiled code rather than a test tree — a `src/tests/scheduler.rs`
/// module in a tool whose own domain is testing, say. There is no
/// reliable, cheap way from the path alone to distinguish "a directory
/// that holds tests" from "a directory that happens to be named tests
/// but holds production code" — semantic content, not spelling, is
/// what actually decides it. Narrowing further (e.g. top-level `tests/`
/// only) is not free either: it would stop excluding
/// `src/other/test/fixture.rs`, the nested shape this module's own test
/// suite already pins as intentionally excluded
/// (`a_test_directory_is_excluded_from_the_search`). Kept as-is,
/// residual accepted and named here rather than silently carried.
fn is_test_path(relative: &str) -> bool {
    relative
        .split('/')
        .any(|seg| seg.eq_ignore_ascii_case("test") || seg.eq_ignore_ascii_case("tests"))
}

/// [`find_literal`]'s result: every matching line, plus every candidate
/// file it could not read as UTF-8 and therefore did not search at all.
/// `corpus.rs` and `marker.rs` tolerate the same non-UTF-8 case silently
/// — there, an unreadable file means one document not indexed or one
/// marker scan skipped, a bounded, locally-visible gap. Here it is
/// different in kind: this search's entire job is proving a *negative*
/// across the whole tree, so a file it could not examine is a hole in
/// the very claim being certified, and a `pass` that rests on an
/// incomplete scan is indistinguishable from a genuine one unless the
/// gap is surfaced. `skipped` is that surfacing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LiteralSearch {
    pub hits: Vec<SourceHit>,
    pub skipped: Vec<String>,
}

/// Search the corpus for `literal` as a plain substring, everywhere
/// except documentation (`.md` — the claim's own document necessarily
/// names the literal to describe its absence, so including it would
/// make every absence claim self-defeating), a recognized test path
/// (`is_test_path`), and any path `git` would not track
/// ([`crate::gitignore::ignored_paths`], one batched query for the whole
/// candidate list) — build output and vendored dependencies are not
/// corpus source, and a hit inside one is neither fixable at the marker
/// nor reproducible machine to machine. A recognized-language file is
/// searched with its comments blanked ([`strip_comments`]); a `.rs` file
/// additionally has every `#[cfg(test)]`/`#[test]`-gated item blanked
/// ([`strip_rust_test_items`]). An unrecognized extension is searched
/// unstripped (`comment_syntax`'s doc comment states why that is the
/// safe default, not an oversight). A file that doesn't decode as UTF-8
/// is not searched, and its path is collected into
/// [`LiteralSearch::skipped`] rather than silently dropped
/// ([`LiteralSearch`]'s own doc states why that distinction matters
/// specifically for this search).
///
/// Returns every matching line, not only the first — `run.rs` reports
/// them all, and a claim backed by several fragment markers (O5's
/// "tile it, name each fragment's location" pattern) benefits from a
/// complete picture per fragment rather than a first-match cutoff.
pub fn find_literal(corpus_root: &Path, literal: &str) -> Result<LiteralSearch, CorpusError> {
    let mut hits = Vec::new();
    let mut skipped = Vec::new();

    // `corpus::walk_files` skips only dot-prefixed entries — it has no
    // notion of `.gitignore` at all, so build output (`target/`) and
    // vendored dependencies (`node_modules/`) would otherwise be
    // searched as corpus source. That is not the tolerable
    // over-reporting direction this feature otherwise accepts (this
    // module's own doc, "Between the two failure directions..."): a hit
    // inside `target/` cannot be fixed at the marker, only by deleting a
    // build directory, and the verdict becomes non-deterministic across
    // machines depending on whether one happened to run `cargo build`.
    // One batched `git check-ignore` for every candidate, mirroring
    // `gitignore::find_unreachable_references`'s own pattern exactly.
    let candidates: Vec<(std::path::PathBuf, String, String)> = corpus::walk_files(corpus_root)?
        .into_iter()
        .filter_map(|path| {
            let relative = path
                .strip_prefix(corpus_root)
                .expect("walk_files only yields paths under corpus_root")
                .to_string_lossy()
                .replace(std::path::MAIN_SEPARATOR, "/");
            let extension = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            if extension == "md" || is_test_path(&relative) {
                return None;
            }
            Some((path, relative, extension))
        })
        .collect();

    let paths: Vec<String> = candidates.iter().map(|(_, r, _)| r.clone()).collect();
    let ignored = crate::gitignore::ignored_paths("git", corpus_root, &paths);

    for (path, relative, extension) in candidates {
        if ignored.contains(&relative) {
            continue;
        }

        let contents = match std::fs::read_to_string(&path) {
            Ok(contents) => contents,
            Err(_) => {
                skipped.push(relative);
                continue;
            }
        };

        let syntax = comment_syntax(&extension);
        let mut searched = strip_comments(&contents, &syntax);
        if extension == "rs" {
            searched = strip_rust_test_items(&searched);
        }

        // Search the whole stripped file, not line by line: a per-line
        // loop can only ever be *equivalent* to this for a `\n`-free
        // literal (any match a per-line `.contains()` finds, a whole-
        // content `match_indices()` finds too, and vice versa, since a
        // literal without an embedded newline can never straddle a
        // `.lines()` boundary) — but a single scan is simpler than
        // materializing every line, and it is the form that also finds
        // a literal that DOES carry an embedded newline should one ever
        // reach here. Line numbers are derived from `searched` itself —
        // never from re-locating the match in the original `contents` —
        // for the reason `blank_into`'s doc comment states: a multi-byte
        // character collapses to one ASCII space, so a byte offset into
        // `searched` does not correspond to the same offset in
        // `contents`. Counting newlines *within* `searched` sidesteps
        // that entirely, because `searched`'s line structure — where
        // every `\n` sits — is preserved exactly from the original file.
        //
        // Line number tracked incrementally rather than re-counting
        // `searched[..offset]` from byte 0 on every hit: `match_indices`
        // yields offsets in increasing order with non-overlapping spans,
        // so counting only the `\n`s between the previous match's start
        // and this one's — then carrying that running total forward —
        // visits each byte of `searched` at most once across the whole
        // loop, rather than once per hit (O(n) total instead of
        // O(n · hits) on a file with many matches).
        let mut last_hit_line: Option<usize> = None;
        let mut line = 1usize;
        let mut counted_upto = 0usize;
        for (offset, _) in searched.match_indices(literal) {
            line += searched[counted_upto..offset].matches('\n').count();
            counted_upto = offset;
            if last_hit_line != Some(line) {
                hits.push(SourceHit {
                    file: relative.clone(),
                    line: Line(line),
                });
                last_hit_line = Some(line);
            }
        }
    }

    hits.sort_by(|a, b| (&a.file, a.line.0).cmp(&(&b.file, b.line.0)));
    skipped.sort();
    Ok(LiteralSearch { hits, skipped })
}

/// A marker naming an `evaluator: absent` claim whose literal is not
/// among that claim's own [`Claim::prose_code`] spans — the drift O3
/// names: the prose (or the marker) changed and the two fell out of
/// step. `Warn` severity in `register.ncl`, never failed on: this
/// diagnostic asserts nothing about whether the underlying absence
/// still holds (that is `run.rs`'s job, over source, at `Fail`
/// severity) — only that the marker may no longer describe anything
/// the document currently says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaleAbsenceMarker {
    pub file: String,
    pub line: Line,
    pub id: ClaimId,
    pub literal: String,
}

/// Only a marker living in the SAME FILE as the claim it names is
/// checked here: staleness is specifically about a marker and its own
/// claim's prose falling out of step with each other, and a marker
/// with no claim in the same file to compare against has no prose
/// scope to check at all. A marker for an `absent` claim id that
/// exists nowhere in the corpus is not "stale" by this check — it is
/// `Outcome::Absent` at `docket run` time, a distinct, load-bearing
/// signal this diagnostic does not duplicate.
pub fn find_stale_markers(corpus: &Corpus, markers: &[Marker]) -> Vec<StaleAbsenceMarker> {
    let absent_claims: Vec<&Claim> = corpus
        .claims
        .iter()
        .filter(|c| c.raw.evaluator.as_deref() == Some("absent"))
        .collect();

    let mut out = Vec::new();
    for claim in absent_claims {
        for m in markers
            .iter()
            .filter(|m| m.id == claim.id && m.file == claim.file)
        {
            // A bare marker (`command: None`, marker.rs's "bare form")
            // carries no literal to compare against the claim's prose —
            // the same reason `run.rs`'s `absent` evaluator never accepts
            // one as a match, only a commanded marker offers a literal an
            // `absent` claim can be discharged (or found stale) by.
            let Some(command) = &m.command else {
                continue;
            };
            if !claim.prose_code.iter().any(|c| c == command) {
                out.push(StaleAbsenceMarker {
                    file: m.file.clone(),
                    line: m.line,
                    id: claim.id.clone(),
                    literal: command.clone(),
                });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extract::extract_document;

    struct TempDir(std::path::PathBuf);
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
            std::fs::write(path, contents).unwrap();
        }
        /// `git init` this directory so `find_literal`'s ignored-path
        /// filter (`gitignore::ignored_paths`) has a real repository to
        /// ask `check-ignore` against — mirrors `corpus.rs`'s own
        /// `TempDir::git_init`.
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
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn tempdir() -> TempDir {
        let dir = std::env::temp_dir().join(format!(
            "docket-absence-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }

    // --- find_literal: the core search ------------------------------

    #[test]
    fn a_literal_present_in_real_source_is_found() {
        let dir = tempdir();
        dir.write("src/lib.rs", "let header = \"Retry-After\";\n");
        let hits = find_literal(dir.path(), "Retry-After").unwrap().hits;
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].file, "src/lib.rs");
        assert_eq!(hits[0].line, Line(1));
    }

    #[test]
    fn a_literal_absent_from_the_whole_tree_reports_no_hits() {
        let dir = tempdir();
        dir.write("src/lib.rs", "let header = \"Content-Length\";\n");
        assert!(
            find_literal(dir.path(), "Retry-After")
                .unwrap()
                .hits
                .is_empty()
        );
    }

    #[test]
    fn documentation_never_counts_as_a_hit() {
        // The self-defeat case: the claim's own doc necessarily names
        // the literal to describe its absence — a `.md` file must never
        // be searched, or every absence claim would fail immediately.
        let dir = tempdir();
        dir.write(
            "docs/specs/x.md",
            "There is no `Retry-After` header on any response.\n",
        );
        assert!(
            find_literal(dir.path(), "Retry-After")
                .unwrap()
                .hits
                .is_empty()
        );
    }

    #[test]
    fn a_test_directory_is_excluded_from_the_search() {
        let dir = tempdir();
        dir.write("tests/integration.rs", "\"Retry-After\"\n");
        dir.write("src/other/test/fixture.rs", "\"Retry-After\"\n");
        assert!(
            find_literal(dir.path(), "Retry-After")
                .unwrap()
                .hits
                .is_empty()
        );
    }

    #[test]
    fn a_real_module_merely_named_tests_is_also_excluded_a_named_residual() {
        // is_test_path's own documented residual, pinned rather than
        // left implicit: a path segment spelled `tests` is excluded
        // even when it names real, always-compiled code (a plausible
        // shape for a tool whose own domain is testing) rather than a
        // test tree. Accepted here — see the doc comment for why
        // narrowing further is not free either.
        let dir = tempdir();
        dir.write("src/tests/scheduler.rs", "\"Retry-After\"\n");
        assert!(
            find_literal(dir.path(), "Retry-After")
                .unwrap()
                .hits
                .is_empty()
        );
    }

    #[test]
    fn a_line_comment_hit_does_not_count() {
        let dir = tempdir();
        dir.write("src/lib.rs", "// TODO: no Retry-After header yet\n");
        assert!(
            find_literal(dir.path(), "Retry-After")
                .unwrap()
                .hits
                .is_empty()
        );
    }

    #[test]
    fn a_block_comment_hit_spanning_lines_does_not_count() {
        let dir = tempdir();
        dir.write(
            "src/lib.rs",
            "/* removed:\nlet h = \"Retry-After\";\n*/\nfn ok() {}\n",
        );
        assert!(
            find_literal(dir.path(), "Retry-After")
                .unwrap()
                .hits
                .is_empty()
        );
    }

    #[test]
    fn real_code_after_a_block_comment_closes_is_still_searched() {
        let dir = tempdir();
        dir.write("src/lib.rs", "/* old note */ let h = \"Retry-After\";\n");
        let hits = find_literal(dir.path(), "Retry-After").unwrap().hits;
        assert_eq!(hits.len(), 1, "{hits:?}");
    }

    #[test]
    fn a_hash_leader_is_a_comment_in_python_but_not_in_rust() {
        // The exact false-positive `#[cfg(test)]`/`#[error(...)]` risk a
        // naive "# starts a comment" rule would create: Rust attributes
        // open with `#` too, so `#` must NOT be a recognized Rust
        // leader, only a Python/shell/TOML one.
        let dir = tempdir();
        dir.write("src/lib.rs", "#[error(\"Retry-After missing\")]\n");
        assert_eq!(
            find_literal(dir.path(), "Retry-After missing")
                .unwrap()
                .hits
                .len(),
            1
        );

        let dir2 = tempdir();
        dir2.write("scripts/note.py", "# Retry-After was removed\n");
        assert!(
            find_literal(dir2.path(), "Retry-After")
                .unwrap()
                .hits
                .is_empty()
        );
    }

    #[test]
    fn an_unrecognized_extension_is_searched_unstripped() {
        // The conservative default (comment_syntax's doc comment):
        // over-reporting, not silent drift.
        let dir = tempdir();
        dir.write("notes.xyz", "# Retry-After, mentioned in passing\n");
        assert_eq!(
            find_literal(dir.path(), "Retry-After").unwrap().hits.len(),
            1
        );
    }

    // --- string-literal-blind comment/brace scanning (the merge gate's
    // three false-`Pass` reproductions, one root cause) ----------------

    #[test]
    fn skip_string_literal_treats_an_escaped_quote_as_not_closing() {
        assert_eq!(
            skip_string_literal(r#""a\"b" tail"#),
            Some(r#""a\"b""#.len())
        );
    }

    #[test]
    fn skip_string_literal_runs_to_the_end_when_unterminated() {
        let rest = "\"never closes";
        assert_eq!(skip_string_literal(rest), Some(rest.len()));
    }

    #[test]
    fn skip_string_literal_is_none_when_rest_does_not_open_a_string() {
        assert_eq!(skip_string_literal("not a string"), None);
    }

    #[test]
    fn a_comment_leader_inside_a_string_literal_is_still_searched() {
        // Reproduced against the built binary: `let s = "// Retry-After";`
        // used to `pass` — the `//` inside the string was read as a real
        // line-comment leader, blanking the literal along with it.
        let dir = tempdir();
        dir.write("src/lib.rs", "let s = \"// Retry-After\";\n");
        let hits = find_literal(dir.path(), "Retry-After").unwrap().hits;
        assert_eq!(hits.len(), 1, "{hits:?}");
    }

    #[test]
    fn a_block_comment_opener_inside_a_string_does_not_open_a_phantom_block() {
        // Reproduced: `"/*";` on one line with no real closing `*/`
        // anywhere in the file used to open a block comment that
        // consumed every line after it, including a genuine
        // `"Retry-After"` on the next line — a phantom comment with no
        // real end, blanking the rest of the file.
        let dir = tempdir();
        dir.write(
            "src/lib.rs",
            "let comment_marker = \"/*\";\nlet real = \"Retry-After\";\n",
        );
        let hits = find_literal(dir.path(), "Retry-After").unwrap().hits;
        assert_eq!(hits.len(), 1, "{hits:?}");
        assert_eq!(hits[0].line, Line(2));
    }

    #[test]
    fn a_brace_inside_a_test_items_string_does_not_swallow_following_real_code() {
        // Reproduced: a `#[test]` body containing a string with one
        // unmatched `{` (`let s = "{";`) undercounts by one; blind brace
        // counting then over-consumed past the test item's true end,
        // synced back up by a coincidental `}` inside a *different*
        // real function's string literal, and blanked that whole
        // function — including its real "Retry-After" — as if it were
        // still part of the test.
        let dir = tempdir();
        dir.write(
            "src/lib.rs",
            "#[test]\nfn t() {\n    let s = \"{\";\n    assert!(true);\n}\n\npub fn error_message() -> &'static str {\n    \"unexpected } here - Retry-After\"\n}\n",
        );
        let hits = find_literal(dir.path(), "Retry-After").unwrap().hits;
        assert_eq!(hits.len(), 1, "{hits:?}");
        assert_eq!(hits[0].line, Line(8));
    }

    #[test]
    fn a_nix_block_comment_hit_does_not_count() {
        let dir = tempdir();
        dir.write("pkg.nix", "/* removed:\n\"Retry-After\"\n*/\n{ }\n");
        assert!(
            find_literal(dir.path(), "Retry-After")
                .unwrap()
                .hits
                .is_empty()
        );
    }

    #[test]
    fn a_sql_block_comment_hit_does_not_count() {
        let dir = tempdir();
        dir.write("q.sql", "/* removed:\n'Retry-After'\n*/\nSELECT 1;\n");
        assert!(
            find_literal(dir.path(), "Retry-After")
                .unwrap()
                .hits
                .is_empty()
        );
    }

    #[test]
    fn a_haskell_block_comment_hit_does_not_count() {
        let dir = tempdir();
        dir.write("M.hs", "{- removed:\n\"Retry-After\"\n-}\nmain = pure ()\n");
        assert!(
            find_literal(dir.path(), "Retry-After")
                .unwrap()
                .hits
                .is_empty()
        );
    }

    #[test]
    fn a_lua_block_comment_hit_does_not_count() {
        let dir = tempdir();
        dir.write(
            "script.lua",
            "--[[ removed:\n\"Retry-After\"\n]]\nprint(1)\n",
        );
        assert!(
            find_literal(dir.path(), "Retry-After")
                .unwrap()
                .hits
                .is_empty()
        );
    }

    #[test]
    fn an_alloy_block_comment_hit_does_not_count() {
        let dir = tempdir();
        dir.write("model.als", "/* removed:\n\"Retry-After\"\n*/\nsig S {}\n");
        assert!(
            find_literal(dir.path(), "Retry-After")
                .unwrap()
                .hits
                .is_empty()
        );
    }

    #[test]
    fn a_cfg_test_module_is_excluded_even_though_it_sits_in_src() {
        let dir = tempdir();
        dir.write(
            "src/lib.rs",
            "fn real() {}\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn old() {\n        assert_eq!(\"Retry-After\", header());\n    }\n}\n",
        );
        assert!(
            find_literal(dir.path(), "Retry-After")
                .unwrap()
                .hits
                .is_empty()
        );
    }

    #[test]
    fn a_standalone_hash_test_function_is_excluded_without_a_cfg_test_module() {
        let dir = tempdir();
        dir.write(
            "src/lib.rs",
            "fn real() {}\n\n#[test]\nfn old() {\n    let x = \"Retry-After\";\n}\n",
        );
        assert!(
            find_literal(dir.path(), "Retry-After")
                .unwrap()
                .hits
                .is_empty()
        );
    }

    #[test]
    fn real_code_after_a_stripped_test_module_is_still_searched() {
        let dir = tempdir();
        dir.write(
            "src/lib.rs",
            "#[cfg(test)]\nmod tests {\n    fn old() {}\n}\n\nlet h = \"Retry-After\";\n",
        );
        let hits = find_literal(dir.path(), "Retry-After").unwrap().hits;
        assert_eq!(hits.len(), 1, "{hits:?}");
    }

    #[test]
    fn a_gitignored_path_is_excluded_from_the_search() {
        // Build output and vendored dependencies are not corpus source
        // (module doc comment above): a hit inside `target/` cannot be
        // fixed at the marker, only by deleting a build directory, and
        // the same commit would verdict differently machine to machine
        // depending on whether `cargo build` had run.
        let dir = tempdir();
        dir.git_init();
        dir.write(".gitignore", "target/\nnode_modules/\n");
        dir.write("target/debug/generated.rs", "\"Retry-After\"\n");
        dir.write("node_modules/dep/index.js", "\"Retry-After\"\n");
        dir.write("src/lib.rs", "let h = \"Content-Length\";\n");
        assert!(
            find_literal(dir.path(), "Retry-After")
                .unwrap()
                .hits
                .is_empty()
        );
    }

    #[test]
    fn find_literal_shares_the_batch_poisoning_fix_not_a_parallel_implementation() {
        // `find_literal` (module code, above) calls
        // `crate::gitignore::ignored_paths` directly — the exact function
        // `gitignore::tests::a_fatal_candidate_falls_back_to_recovering_every_other_one`
        // already pins against the real defect: one candidate `git
        // check-ignore --stdin` treats as an invalid pathspec (`/`, `..`,
        // a leading `//`) used to fatal the WHOLE batch, silently
        // discarding every real match beside it — which here would mean
        // a false `pass` on an absence claim whose literal is actually
        // present in an ignored-but-unfiltered directory, exactly the
        // failure `evaluator: absent` exists to prevent.
        //
        // This is a wiring proof, not a reproduction from `find_literal`'s
        // own inputs: unlike `gitignore.rs`'s code-references (free text
        // from inside a document, which genuinely can be `/` or `..`),
        // `find_literal`'s candidates come from `corpus::walk_files` —
        // real relative filesystem paths, which can never literally BE
        // `/`, `..`, or empty (none is a constructible file or directory
        // name). Confirmed directly: a filename containing a literal
        // newline byte (a real, if unusual, possibility on this
        // filesystem) does NOT trigger the same 128-fatal path either —
        // git silently treats it as an extra stdin line rather than an
        // invalid pathspec. So there is no organic way to reproduce the
        // exact defect through this function's own candidate-generation
        // path; what is verified here is that it depends on the SAME
        // fixed function, not a parallel, unfixed one.
        let repo = tempdir();
        repo.git_init();
        repo.write(".gitignore", "target/\n");
        repo.write("target/debug/generated.rs", "");

        let ignored = crate::gitignore::ignored_paths(
            "git",
            repo.path(),
            &[
                "target/debug/generated.rs".to_string(),
                "/".to_string(),
                "src/lib.rs".to_string(),
            ],
        );
        assert!(
            ignored.contains("target/debug/generated.rs"),
            "the real ignored path must still be found even with a fatal \
             candidate in the same batch: {ignored:?}"
        );
        assert!(!ignored.contains("src/lib.rs"));
    }

    #[test]
    fn a_literal_the_source_genuinely_splits_across_a_line_break_is_not_found() {
        // Not a regression case for a real defect: a `\n`-free literal
        // can never straddle a `.lines()` boundary (the newline that
        // separates the two lines would itself have to be part of the
        // literal), so a per-line scan and a whole-content scan agree
        // here by construction. Rust's own backslash line-continuation
        // is the case that looks like a counterexample and isn't: the
        // *compiled* value is "Retry-After header", but the *raw source
        // bytes* `find_literal` actually searches hold `\` + `\n` +
        // leading indentation between the words, not a single space —
        // so the literal genuinely does not occur verbatim, and `Pass`
        // (no hit) is the correct verdict, matching MVP.md's contract
        // ("searched for, verbatim, across the corpus's non-documentation
        // source"). Pinned here so a future change to the search loop
        // does not start reporting a false `Fail` on this shape.
        let dir = tempdir();
        dir.write(
            "src/lib.rs",
            "pub const MSG: &str = \"Retry-After \\\n    header\";\n",
        );
        assert!(
            find_literal(dir.path(), "Retry-After header")
                .unwrap()
                .hits
                .is_empty()
        );
    }

    #[test]
    fn a_hit_after_a_multi_byte_comment_reports_the_correct_line_number() {
        // The trap a byte-offset-based line count would fall into:
        // `blank_into` collapses every multi-byte character in a comment
        // to a single ASCII space, so a byte offset into `searched` does
        // NOT correspond to the same offset in the original file. Line
        // number must come from counting `\n` within `searched` itself
        // (whose line structure mirrors the original exactly), never
        // from re-locating the match's offset in `contents`.
        let dir = tempdir();
        dir.write(
            "src/lib.rs",
            "// 日本語のコメント\nfn ok() {}\nlet h = \"Retry-After\";\n",
        );
        let hits = find_literal(dir.path(), "Retry-After").unwrap().hits;
        assert_eq!(hits.len(), 1, "{hits:?}");
        assert_eq!(hits[0].line, Line(3));
    }

    #[test]
    fn multiple_hits_across_files_are_all_reported() {
        let dir = tempdir();
        dir.write("src/a.rs", "\"Retry-After\"\n");
        dir.write("src/b.rs", "\"Retry-After\"\n");
        let hits = find_literal(dir.path(), "Retry-After").unwrap().hits;
        assert_eq!(hits.len(), 2, "{hits:?}");
    }

    #[test]
    fn a_non_utf8_file_is_not_searched_but_is_named_in_skipped() {
        // This search proves a negative across the whole tree
        // (`LiteralSearch`'s own doc): an unreadable file is a gap in
        // the very claim being certified, so it is reported, not
        // silently dropped the way corpus.rs/marker.rs tolerate the
        // same case elsewhere.
        let dir = tempdir();
        dir.write("src/ok.rs", "\"Retry-After\"\n");
        dir.write_bytes("src/binary.rs", &[0xff, 0xfe, 0x00, 0x01]);
        let search = find_literal(dir.path(), "Retry-After").unwrap();
        assert_eq!(search.hits.len(), 1);
        assert_eq!(search.hits[0].file, "src/ok.rs");
        assert_eq!(search.skipped, vec!["src/binary.rs".to_string()]);
    }

    #[test]
    fn a_clean_search_with_nothing_unreadable_reports_no_skipped_paths() {
        let dir = tempdir();
        dir.write("src/ok.rs", "\"Content-Length\"\n");
        let search = find_literal(dir.path(), "Retry-After").unwrap();
        assert!(search.hits.is_empty());
        assert!(search.skipped.is_empty());
    }

    // --- find_stale_markers -------------------------------------------

    fn corpus_with(file: &str, src: &str) -> Corpus {
        let result = extract_document(file, src);
        Corpus {
            claims: result.claims,
            documents: Vec::new(),
        }
    }

    fn marker_at(file: &str, id: &str, command: &str, line: usize) -> Marker {
        Marker {
            id: id.to_string(),
            command: Some(command.to_string()),
            file: file.to_string(),
            line: Line(line),
            exempt: false,
        }
    }

    #[test]
    fn a_marker_whose_literal_is_still_in_the_claims_prose_is_not_stale() {
        let corpus = corpus_with(
            "docs/specs/x.md",
            "### [x]\n\nThere is no `Retry-After` header.\n\n```claim\nkind: constraint\nevaluator: absent\n```\n",
        );
        let markers = vec![marker_at("docs/specs/x.md", "x", "Retry-After", 3)];
        assert!(find_stale_markers(&corpus, &markers).is_empty());
    }

    #[test]
    fn a_marker_whose_literal_the_prose_no_longer_mentions_is_stale() {
        let corpus = corpus_with(
            "docs/specs/x.md",
            "### [x]\n\nThe old note about it is gone now.\n\n```claim\nkind: constraint\nevaluator: absent\n```\n",
        );
        let markers = vec![marker_at("docs/specs/x.md", "x", "Retry-After", 3)];
        let stale = find_stale_markers(&corpus, &markers);
        assert_eq!(stale.len(), 1, "{stale:?}");
        assert_eq!(stale[0].literal, "Retry-After");
        assert_eq!(stale[0].id, "x");
    }

    #[test]
    fn a_marker_in_a_different_file_from_its_claim_is_not_checked() {
        let corpus = corpus_with(
            "docs/specs/x.md",
            "### [x]\n\nThere is no `Retry-After` header.\n\n```claim\nkind: constraint\nevaluator: absent\n```\n",
        );
        // Same id, but the marker lives in src/, not the claim's own
        // document — no prose scope to compare it against.
        let markers = vec![marker_at("src/lib.rs", "x", "Retry-After", 1)];
        assert!(find_stale_markers(&corpus, &markers).is_empty());
    }

    #[test]
    fn a_non_absent_claim_is_never_checked_for_staleness() {
        let corpus = corpus_with(
            "docs/specs/x.md",
            "### [x]\n\nNo backtick code span mentions the marker's literal here.\n\n```claim\nkind: constraint\nevaluator: test\n```\n",
        );
        let markers = vec![marker_at("docs/specs/x.md", "x", "cargo test x", 3)];
        assert!(find_stale_markers(&corpus, &markers).is_empty());
    }

    #[test]
    fn a_bare_marker_on_an_absent_claim_is_never_stale() {
        // marker.rs's bare form (`command: None`) carries no literal to
        // compare against the claim's prose — meaningless for this check,
        // the same way run.rs's `absent` evaluator never accepts a bare
        // marker as a match. Confirms the merge with feat/bare-marker
        // (which introduced `Marker::command: Option<String>`) didn't
        // leave this check panicking or misreading `None` as a literal.
        let corpus = corpus_with(
            "docs/specs/x.md",
            "### [x]\n\nThere is no `Retry-After` header.\n\n```claim\nkind: constraint\nevaluator: absent\n```\n",
        );
        let markers = vec![Marker {
            id: "x".to_string(),
            command: None,
            file: "docs/specs/x.md".to_string(),
            line: Line(3),
            exempt: false,
        }];
        assert!(find_stale_markers(&corpus, &markers).is_empty());
    }

    #[test]
    fn a_literal_appearing_only_inside_a_sub_heading_does_not_suppress_staleness() {
        // extract.rs's `prose_code` must not fold a sub-heading's own
        // code span into a claim's prose-mention set: the sub-heading
        // sits within the claim's scope (only a same-or-higher-level
        // heading ends it), but a code span living in a heading is not
        // prose *about* the claim's absence. Before the fix, this
        // sub-heading's `` `Retry-After` `` counted as a prose mention
        // and wrongly suppressed the warning.
        let corpus = corpus_with(
            "docs/specs/x.md",
            "### [x]\n\nThe old note about it is gone now.\n\n#### `Retry-After` (historical)\n\nSome detail.\n\n```claim\nkind: constraint\nevaluator: absent\n```\n",
        );
        let markers = vec![marker_at("docs/specs/x.md", "x", "Retry-After", 3)];
        let stale = find_stale_markers(&corpus, &markers);
        assert_eq!(stale.len(), 1, "{stale:?}");
        assert_eq!(stale[0].literal, "Retry-After");
    }
}
