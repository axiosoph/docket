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
        "py" | "sh" | "bash" | "zsh" | "rb" | "toml" | "yaml" | "yml" | "ncl" | "nix" | "pl" => {
            CommentSyntax {
                line: Some("#"),
                block: None,
            }
        }
        "tla" => CommentSyntax {
            line: Some("\\*"),
            block: Some(("(*", "*)")),
        },
        "lean" => CommentSyntax {
            line: Some("--"),
            block: Some(("/-", "-/")),
        },
        "als" | "sql" | "hs" | "lua" | "adb" | "ads" => CommentSyntax {
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

/// Blank every comment in `contents` to spaces, line count and every
/// newline preserved exactly — so a hit's line number, computed
/// afterward by simple line indexing, still points at the original
/// file. Heuristic, not a real lexer for any of the languages `syntax`
/// covers: a leader or block-opener inside a string or char literal is
/// not distinguished from a real comment (design residue, O5: the same
/// trade-off this project already accepts for `run.rs`'s vacuity
/// signals — a substring test, not a parser).
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
/// after it — ASCII brace-counted, not a real parser (same heuristic
/// class as [`strip_comments`]: a brace inside a string or char literal
/// is not distinguished from a real one). `None` if no `{` appears, or
/// braces never balance, before end of input.
fn brace_span(s: &str) -> Option<usize> {
    let open = s.find('{')?;
    let mut depth = 0i32;
    for (i, ch) in s[open..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(open + i + 1);
                }
            }
            _ => {}
        }
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
fn is_test_path(relative: &str) -> bool {
    relative
        .split('/')
        .any(|seg| seg.eq_ignore_ascii_case("test") || seg.eq_ignore_ascii_case("tests"))
}

/// Search the corpus for `literal` as a plain substring, everywhere
/// except documentation (`.md` — the claim's own document necessarily
/// names the literal to describe its absence, so including it would
/// make every absence claim self-defeating) and a recognized test path
/// (`is_test_path`). A recognized-language file is searched with its
/// comments blanked ([`strip_comments`]); a `.rs` file additionally has
/// every `#[cfg(test)]`/`#[test]`-gated item blanked
/// ([`strip_rust_test_items`]). An unrecognized extension is searched
/// unstripped (`comment_syntax`'s doc comment states why that is the
/// safe default, not an oversight). A file that doesn't decode as UTF-8
/// is skipped, the same tolerance `marker.rs` gives a binary artifact
/// sitting inside the walked tree.
///
/// Returns every matching line, not only the first — `run.rs` reports
/// them all, and a claim backed by several fragment markers (O5's
/// "tile it, name each fragment's location" pattern) benefits from a
/// complete picture per fragment rather than a first-match cutoff.
pub fn find_literal(corpus_root: &Path, literal: &str) -> Result<Vec<SourceHit>, CorpusError> {
    let mut hits = Vec::new();

    for path in corpus::walk_files(corpus_root)? {
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
        if extension == "md" {
            continue;
        }
        if is_test_path(&relative) {
            continue;
        }

        let Ok(contents) = std::fs::read_to_string(&path) else {
            continue;
        };

        let syntax = comment_syntax(&extension);
        let mut searched = strip_comments(&contents, &syntax);
        if extension == "rs" {
            searched = strip_rust_test_items(&searched);
        }

        for (i, line) in searched.lines().enumerate() {
            if line.contains(literal) {
                hits.push(SourceHit {
                    file: relative.clone(),
                    line: Line(i + 1),
                });
            }
        }
    }

    hits.sort_by(|a, b| (&a.file, a.line.0).cmp(&(&b.file, b.line.0)));
    Ok(hits)
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
        let hits = find_literal(dir.path(), "Retry-After").unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].file, "src/lib.rs");
        assert_eq!(hits[0].line, Line(1));
    }

    #[test]
    fn a_literal_absent_from_the_whole_tree_reports_no_hits() {
        let dir = tempdir();
        dir.write("src/lib.rs", "let header = \"Content-Length\";\n");
        assert!(find_literal(dir.path(), "Retry-After").unwrap().is_empty());
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
        assert!(find_literal(dir.path(), "Retry-After").unwrap().is_empty());
    }

    #[test]
    fn a_test_directory_is_excluded_from_the_search() {
        let dir = tempdir();
        dir.write("tests/integration.rs", "\"Retry-After\"\n");
        dir.write("src/other/test/fixture.rs", "\"Retry-After\"\n");
        assert!(find_literal(dir.path(), "Retry-After").unwrap().is_empty());
    }

    #[test]
    fn a_line_comment_hit_does_not_count() {
        let dir = tempdir();
        dir.write("src/lib.rs", "// TODO: no Retry-After header yet\n");
        assert!(find_literal(dir.path(), "Retry-After").unwrap().is_empty());
    }

    #[test]
    fn a_block_comment_hit_spanning_lines_does_not_count() {
        let dir = tempdir();
        dir.write(
            "src/lib.rs",
            "/* removed:\nlet h = \"Retry-After\";\n*/\nfn ok() {}\n",
        );
        assert!(find_literal(dir.path(), "Retry-After").unwrap().is_empty());
    }

    #[test]
    fn real_code_after_a_block_comment_closes_is_still_searched() {
        let dir = tempdir();
        dir.write("src/lib.rs", "/* old note */ let h = \"Retry-After\";\n");
        let hits = find_literal(dir.path(), "Retry-After").unwrap();
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
                .len(),
            1
        );

        let dir2 = tempdir();
        dir2.write("scripts/note.py", "# Retry-After was removed\n");
        assert!(find_literal(dir2.path(), "Retry-After").unwrap().is_empty());
    }

    #[test]
    fn an_unrecognized_extension_is_searched_unstripped() {
        // The conservative default (comment_syntax's doc comment):
        // over-reporting, not silent drift.
        let dir = tempdir();
        dir.write("notes.xyz", "# Retry-After, mentioned in passing\n");
        assert_eq!(find_literal(dir.path(), "Retry-After").unwrap().len(), 1);
    }

    #[test]
    fn a_cfg_test_module_is_excluded_even_though_it_sits_in_src() {
        let dir = tempdir();
        dir.write(
            "src/lib.rs",
            "fn real() {}\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn old() {\n        assert_eq!(\"Retry-After\", header());\n    }\n}\n",
        );
        assert!(find_literal(dir.path(), "Retry-After").unwrap().is_empty());
    }

    #[test]
    fn a_standalone_hash_test_function_is_excluded_without_a_cfg_test_module() {
        let dir = tempdir();
        dir.write(
            "src/lib.rs",
            "fn real() {}\n\n#[test]\nfn old() {\n    let x = \"Retry-After\";\n}\n",
        );
        assert!(find_literal(dir.path(), "Retry-After").unwrap().is_empty());
    }

    #[test]
    fn real_code_after_a_stripped_test_module_is_still_searched() {
        let dir = tempdir();
        dir.write(
            "src/lib.rs",
            "#[cfg(test)]\nmod tests {\n    fn old() {}\n}\n\nlet h = \"Retry-After\";\n",
        );
        let hits = find_literal(dir.path(), "Retry-After").unwrap();
        assert_eq!(hits.len(), 1, "{hits:?}");
    }

    #[test]
    fn multiple_hits_across_files_are_all_reported() {
        let dir = tempdir();
        dir.write("src/a.rs", "\"Retry-After\"\n");
        dir.write("src/b.rs", "\"Retry-After\"\n");
        let hits = find_literal(dir.path(), "Retry-After").unwrap();
        assert_eq!(hits.len(), 2, "{hits:?}");
    }

    #[test]
    fn a_non_utf8_file_is_skipped_without_erroring() {
        let dir = tempdir();
        dir.write("src/ok.rs", "\"Retry-After\"\n");
        dir.write_bytes("src/binary.rs", &[0xff, 0xfe, 0x00, 0x01]);
        let hits = find_literal(dir.path(), "Retry-After").unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].file, "src/ok.rs");
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
}
