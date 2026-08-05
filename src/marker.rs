//! Evaluator markers: `@docket: <claim-id> :: <command>` lines, found
//! anywhere in the corpus tree.
//!
//! MVP.md's claim block names only an evaluator *kind* (`test`,
//! `proof`, …) — never a location, and the runner (`run.rs`) must not
//! force one into the block: README.md already argues, for the
//! adjacent coverage-index feature, that the claim-block-to-evaluator
//! link belongs beside the evaluator rather than the document ("tests
//! churn far more than claims, so the reference belongs in the artifact
//! that moves") and states the claim block needs no change to support
//! it. That reasoning applies here without alteration, so this runner
//! reuses it rather than inventing a competing mechanism (e.g. a `run:`
//! field on the claim block, which would be exactly the kind of
//! evaluator-in-the-document coupling the churn argument rejects).
//!
//! **This marker grammar is a considered extension of, not identical
//! to, README.md's illustrative example** (`// @docket: lock-groundness`
//! with nothing after it). That example locates an evaluator for
//! *coverage counting* — "does something discharge this claim" — which
//! only needs existence. Running an evaluator needs more: proof,
//! model-check, type, property-test, test and example evaluators are
//! invoked five different ways in three different languages (Rust
//! tests, TLA+ model checking, Alloy analysis, at minimum — see the
//! runner dispatch), and only the marker's own author — the person who
//! wrote the Lean theorem, the TLA+ module, or the Rust test — actually
//! knows the right invocation. So the marker carries it explicitly:
//!
//! ```text
//! // @docket: lock-groundness :: cargo test ground_values_only -- --exact
//! \* @docket: spine-chain-complete :: tlc -config Model.cfg Model.tla
//! -- @docket: no-double-spend :: alloy exec -c Model.als NoDoubleSpend
//! ```
//!
//! One grammar, three comment leaders — the *leader* is never parsed at
//! all (README.md's own point: "a corpus's evaluators are not all one
//! language... a line comment can mark all of them"). This scanner
//! looks for the literal text `@docket:` anywhere on a line, which is
//! exactly as language-agnostic and sidesteps writing a comment lexer
//! for every language a corpus's evaluators happen to be in.
//!
//! **The id may carry a trailing `!`**, immediately after it and before
//! any whitespace: `@docket: <id>! :: <command>`. This exempts the
//! marker from the runner's vacuity detection (run.rs) — its exit
//! status alone is trusted, unconditionally. It exists for an evaluator
//! kind the runner has no output recognizer for (README.md's own list —
//! Lean, TLA+, Alloy — has three, and the runner starts with a
//! recognizer for exactly one, cargo's), so that evaluator would
//! otherwise be permanently unable to report a genuine `Pass`: no
//! recognizer to clear it, no way to say "trust me." A **deliberate**
//! per-marker assertion the author writes once, not a default any
//! marker gets silently — see run.rs's module docs for why that
//! distinction is load-bearing rather than cosmetic.
//!
//! **Alternatives weighed and rejected** (see the runner dispatch,
//! "Weigh at least..."):
//!
//! - *A command string on the claim block itself.* Rejected: this is
//!   the exact coupling README.md's churn argument already rejects for
//!   the sibling feature, and MVP.md's acceptance criteria require this
//!   repository's own claims to still validate unchanged if the schema
//!   isn't touched — it isn't.
//! - *A path on the claim block, with the runner inferred from the
//!   file's language/extension.* Rejected: infeasible without also
//!   parsing each language well enough to find "the next runnable
//!   thing" (a Rust test function's name; a TLA+ module's `.cfg`
//!   pairing; an Alloy assertion name) — exactly the
//!   does-not-survive-contact risk the dispatch names. The marker
//!   carrying the literal command sidesteps all per-language parsing
//!   entirely: the author already knows how to run their own evaluator.
//! - *A per-genre/per-corpus tier→invocation mapping in `docket.ncl`.*
//!   Rejected for the same reason plus one more: it would need a new
//!   `docket.ncl` field, and this repository's own `docket.ncl` is out
//!   of this dispatch's edit surface — a design that needs an
//!   out-of-surface file changed to exercise itself is a design this
//!   corpus specifically cannot dogfood.

use crate::corpus::{CorpusError, walk_files};
use crate::model::{ClaimId, Line};
use std::path::Path;

/// One `@docket: <id> :: <command>` marker, located.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Marker {
    pub id: ClaimId,
    pub command: String,
    pub file: String,
    pub line: Line,
    /// `@docket: <id>! :: <command>` — a deliberate, per-marker assertion
    /// that this command's success really does mean something was
    /// checked, so the runner's vacuity detection (run.rs) must not be
    /// applied to it. Exists for the evaluator kind vacuity detection
    /// cannot cover (no recognized signal in its output) but the author
    /// knows is conclusive; see run.rs's module docs for why this has to
    /// be opt-out rather than a silent default, and why it must be
    /// spelled explicitly rather than inferred.
    pub exempt: bool,
}

const KEYWORD: &str = "@docket:";

/// Whether `s` is a non-empty run of kebab-case segments — the same
/// grammar `extract::bracket_kebab_id` enforces for `[id]` headings,
/// applied here without the brackets (a marker's id has no delimiter of
/// its own; the token simply ends where kebab-case characters stop).
fn is_kebab_case(s: &str) -> bool {
    !s.is_empty()
        && s.split('-').all(|seg| {
            !seg.is_empty()
                && seg
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        })
}

/// `@docket:` preceded by nothing, or by a byte that isn't itself part of
/// a longer identifier — so a marker is recognized whether it opens a
/// line comment (`// @docket: …`) or follows one (`\* @docket: …`), while
/// `not_@docket:` (part of some other identifier) is not mistaken for
/// one. Returns the byte offset of the match.
fn find_keyword(line: &str) -> Option<usize> {
    let mut from = 0;
    while let Some(rel) = line[from..].find(KEYWORD) {
        let pos = from + rel;
        let boundary_ok = match line.as_bytes().get(pos.wrapping_sub(1)) {
            None => true,
            Some(b) => !(b.is_ascii_alphanumeric() || *b == b'_'),
        };
        if boundary_ok {
            return Some(pos);
        }
        from = pos + KEYWORD.len();
    }
    None
}

/// Parse one line for a marker. `None` covers both "no `@docket:` on this
/// line at all" and "`@docket:` is present but not followed by
/// `<kebab-id> :: <command>`" — including a bare `// @docket: <id>` with
/// no `::` suffix (README.md's illustrative form for the separate
/// coverage-index feature, or just not a marker). Both are equally
/// invisible to the runner: a marker this parser cannot execute is
/// indistinguishable, to the runner, from no marker at all.
///
/// The id may carry a trailing `!` (no intervening whitespace) marking
/// it vacuity-exempt — `@docket: <id>! :: <command>` — the author's
/// explicit assertion that this command's exit status alone is
/// conclusive (run.rs).
fn parse_marker_line(line: &str) -> Option<(ClaimId, bool, String)> {
    let pos = find_keyword(line)?;
    let after = line[pos + KEYWORD.len()..].trim_start();

    let id_len = after
        .find(|c: char| !(c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'))
        .unwrap_or(after.len());
    let id = &after[..id_len];
    if !is_kebab_case(id) {
        return None;
    }

    let (exempt, rest) = match after[id_len..].strip_prefix('!') {
        Some(rest) => (true, rest),
        None => (false, &after[id_len..]),
    };

    let command = rest.trim_start().strip_prefix("::")?.trim();
    if command.is_empty() {
        return None;
    }

    Some((id.to_string(), exempt, command.to_string()))
}

/// Scan every file under `root` (the same walk `corpus::load_corpus`
/// uses, minus its `.md`-only filter — an evaluator marker lives in
/// source, not documentation) for `@docket:` markers.
///
/// A file that doesn't decode as UTF-8 is skipped, not an error — the
/// same tolerance `corpus.rs` documents for a binary model-checker dump
/// sitting inside a matched genre, generalized: most of what this walk
/// crosses (build output, generated artifacts) is exactly the kind of
/// thing that is not itself an evaluator's source.
pub fn scan_markers(root: &Path) -> Result<Vec<Marker>, CorpusError> {
    let mut markers = Vec::new();

    for path in walk_files(root)? {
        let Ok(contents) = std::fs::read_to_string(&path) else {
            continue;
        };
        let relative = path
            .strip_prefix(root)
            .expect("walk_files only yields paths under root")
            .to_string_lossy()
            .replace(std::path::MAIN_SEPARATOR, "/");

        for (i, line) in contents.lines().enumerate() {
            if let Some((id, exempt, command)) = parse_marker_line(line) {
                markers.push(Marker {
                    id,
                    command,
                    file: relative.clone(),
                    line: Line(i + 1),
                    exempt,
                });
            }
        }
    }

    markers.sort_by(|a, b| (&a.file, a.line.0).cmp(&(&b.file, b.line.0)));
    Ok(markers)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_rust_line_comment_marker() {
        assert_eq!(
            parse_marker_line("// @docket: lock-groundness :: cargo test ground_values_only"),
            Some((
                "lock-groundness".to_string(),
                false,
                "cargo test ground_values_only".to_string()
            ))
        );
    }

    #[test]
    fn parses_a_tla_plus_line_comment_marker() {
        assert_eq!(
            parse_marker_line("\\* @docket: spine-chain-complete :: tlc Model.tla"),
            Some((
                "spine-chain-complete".to_string(),
                false,
                "tlc Model.tla".to_string()
            ))
        );
    }

    #[test]
    fn parses_an_alloy_line_comment_marker() {
        assert_eq!(
            parse_marker_line("-- @docket: no-double-spend :: alloy exec Model.als"),
            Some((
                "no-double-spend".to_string(),
                false,
                "alloy exec Model.als".to_string()
            ))
        );
    }

    #[test]
    fn a_trailing_bang_on_the_id_marks_the_marker_vacuity_exempt() {
        assert_eq!(
            parse_marker_line("// @docket: no-double-spend! :: alloy exec Model.als"),
            Some((
                "no-double-spend".to_string(),
                true,
                "alloy exec Model.als".to_string()
            ))
        );
    }

    #[test]
    fn the_exempt_bang_must_immediately_follow_the_id_not_float_in_whitespace() {
        // A `!` with a space before it is not part of this grammar at
        // all — it fails the `::`-prefix check the same way any other
        // stray token would, rather than being silently absorbed as
        // exempt.
        assert_eq!(
            parse_marker_line("// @docket: no-double-spend ! :: alloy exec Model.als"),
            None
        );
    }

    #[test]
    fn a_bare_marker_with_no_command_does_not_parse() {
        // README.md's own illustrative form — deliberately not this
        // runner's grammar (module docs). Confirms it is silently
        // invisible rather than a malformed-marker error: a claim
        // backed only by this form reports `absent`, not a parse
        // failure.
        assert_eq!(parse_marker_line("// @docket: lock-groundness"), None);
    }

    #[test]
    fn a_line_with_no_keyword_at_all_does_not_parse() {
        assert_eq!(parse_marker_line("fn ground_values_only() {}"), None);
    }

    #[test]
    fn docket_as_part_of_a_longer_identifier_is_not_a_keyword_match() {
        assert_eq!(
            parse_marker_line("// not_@docket: x :: y"),
            None,
            "the boundary check must reject a `@docket:` that is part of a longer token"
        );
    }

    #[test]
    fn an_uppercase_or_non_kebab_id_does_not_parse() {
        assert_eq!(
            parse_marker_line("// @docket: Lock_Groundness :: cmd"),
            None
        );
    }

    #[test]
    fn a_command_with_double_colons_in_it_keeps_only_the_first_split() {
        // `::` is common in the commands this marker carries (Rust path
        // syntax, cargo's own `--`-adjacent flags never contain `::`,
        // but a Rust test filter like `mod::test_name` does) — the
        // split must not require `::` to be the *only* occurrence.
        assert_eq!(
            parse_marker_line("// @docket: x :: cargo test mod::test_name -- --exact"),
            Some((
                "x".to_string(),
                false,
                "cargo test mod::test_name -- --exact".to_string()
            ))
        );
    }

    #[test]
    fn scans_a_small_tree_and_reports_file_and_line() {
        let dir = tempdir();
        dir.write("src/lib.rs", "// nothing here\n// @docket: x :: true\n");
        dir.write("docs/notes.md", "no markers\n");
        dir.write(".git/config", "// @docket: hidden :: true\n");

        let markers = scan_markers(dir.path()).unwrap();
        assert_eq!(markers.len(), 1);
        assert_eq!(markers[0].id, "x");
        assert_eq!(markers[0].command, "true");
        assert_eq!(markers[0].file, "src/lib.rs");
        assert_eq!(markers[0].line, Line(2));
    }

    #[test]
    fn skips_a_non_utf8_file_without_erroring() {
        let dir = tempdir();
        dir.write("src/marked.rs", "// @docket: x :: true\n");
        dir.write_bytes("src/binary.bin", &[0xff, 0xfe, 0x00, 0x01]);

        let markers = scan_markers(dir.path()).unwrap();
        assert_eq!(markers.len(), 1);
        assert_eq!(markers[0].file, "src/marked.rs");
    }

    #[test]
    fn multiple_markers_for_distinct_ids_are_all_found() {
        let dir = tempdir();
        dir.write(
            "src/lib.rs",
            "// @docket: a :: true\nfn x() {}\n// @docket: b :: false\n",
        );

        let markers = scan_markers(dir.path()).unwrap();
        let ids: Vec<&str> = markers.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, vec!["a", "b"]);
    }

    // --- test helpers, matching corpus.rs's own minimal temp-dir shape --

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
            "docket-marker-test-{}-{}",
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
