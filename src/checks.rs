//! The register evaluator's Rust side: build the corpus+config into the
//! data shape `contracts/register.ncl` expects, invoke it once, and
//! convert the validated result back into typed Rust values.
//!
//! `.ledger/2026-07-30-reference-kinds-and-document-resolution.md`, R6:
//! "everything downstream of 'here is the set of claims with their typed
//! references' is pure and belongs in Nickel." C1-C5, `orphan-claim`,
//! `orphaned-because`, `normative-prose`, `unregistered-definition`,
//! `malformed-id`, and the index projection (formerly `index.rs`) all
//! live in `contracts/register.ncl` now; this module is the seam —
//! serialize, invoke, deserialize — not a reimplementation of any check.
//!
//! **A finding the migration estimate's own line-count table didn't
//! anticipate**: `model::CiteRef`/`RefKind`/`Claim::refs`/`anchor_matches`
//! could not move to Nickel with everything else, because `blast.rs` and
//! `main.rs`'s `run_blast` — both explicitly out of this migration's
//! scope — depend on them directly (`Corpus.claims[].depends: Vec<CiteRef>`,
//! `CiteRef::to_string()` as blast's edge key). Moving them would have
//! meant redesigning blast.rs's graph walk too, which the dispatch
//! explicitly reserves as separate work. Resolved by leaving model.rs's
//! shared infrastructure untouched and having `register.ncl`
//! independently re-derive ref parsing and anchor matching over
//! `CiteRef::to_string()`'s already-round-tripped strings — the honest
//! cost of R6's boundary meeting blast.rs's no-touch scope, not a defect
//! introduced here.

use crate::absence;
use crate::config::Config;
use crate::corpus::LoadedCorpus;
use crate::marker::Marker;
use crate::model::{Index, IndexClaim, IndexDocument};
use crate::nickel::{self, NickelError};
use serde::Serialize;
use std::path::Path;

/// `register.ncl`'s path relative to the crate root. `run_checks` itself
/// takes an already-resolved path — the CLI defaults it to docket's own
/// embedded copy (`contracts.rs`, `--register` overrides it) rather than
/// resolving this string against the current directory, since the
/// evaluator ships with the tool, not with a corpus. This constant
/// survives only to let tests load the real `register.ncl`, exercising
/// the actual evaluator rather than a double — valid because `cargo
/// test`'s cwd is the crate root, i.e. docket's own checkout.
#[cfg(test)]
const DEFAULT_REGISTER_RELATIVE_PATH: &str = "contracts/register.ncl";

#[derive(Debug, thiserror::Error)]
pub enum RegisterError {
    #[error(transparent)]
    Nickel(#[from] NickelError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Fail,
    Warn,
}

/// MVP.md §3: "Each failure names the file, the line, and the offending
/// value." Despite the name, a `Diagnostic` is not always a failure —
/// `severity` says which. `check` is register.ncl's own identifier
/// string ("C1", "orphan-claim", …) rather than a Rust enum: the set of
/// checks is now declared in Nickel, and Rust has no exhaustiveness
/// obligation over it to justify re-declaring the vocabulary here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub check: String,
    pub severity: Severity,
    pub file: String,
    pub line: usize,
    pub message: String,
}

#[derive(Debug, Clone, Default)]
pub struct CheckReport {
    pub diagnostics: Vec<Diagnostic>,
}

impl CheckReport {
    /// True iff nothing at `Fail` severity fired — a report holding only
    /// `Warn`-severity diagnostics still passes (MVP.md §5, exit 0).
    pub fn passed(&self) -> bool {
        !self
            .diagnostics
            .iter()
            .any(|d| d.severity == Severity::Fail)
    }
}

/// The register evaluator's full result: the index (MVP.md §4.1) and
/// every check's diagnostics, both produced by the same Nickel
/// evaluation.
#[derive(Debug, Clone)]
pub struct RegisterResult {
    pub index: Index,
    pub report: CheckReport,
}

// --- input payload: corpus + config, as register.ncl's `Input` -----------

#[derive(Serialize)]
struct InputClaim {
    id: String,
    file: String,
    heading_line: usize,
    block_line: usize,
    /// The claim block's raw YAML, re-parsed to a JSON value — what C1
    /// validates. `None` (with `raw_error` set) if the YAML itself
    /// doesn't parse; that is a C1 violation in its own right (MVP.md
    /// §3), not a distinct Rust error, the same treatment the per-claim
    /// contract call this replaces already gave it.
    raw_value: Option<serde_json::Value>,
    raw_error: Option<String>,
    /// `Claim::refs`' round-tripped canonical strings
    /// (`CiteRef::Display`), not the block's raw YAML sequence: the
    /// best-effort parse into `CiteRef` already happened in extract.rs
    /// (needed unchanged by blast.rs), so this is a lossless handoff of
    /// that result, not a second extraction.
    depends: Vec<String>,
    because: Vec<String>,
    prose_links: Vec<String>,
}

#[derive(Serialize)]
struct InputHeading {
    level: u8,
    text: String,
    line: usize,
}

#[derive(Serialize)]
struct InputDocument {
    doc_path: String,
    file: String,
    genre_path: String,
    headings: Vec<InputHeading>,
}

#[derive(Serialize)]
struct InputGenre {
    path: String,
    kinds: Vec<String>,
}

#[derive(Serialize)]
struct InputOrphanClaim {
    file: String,
    line: usize,
}

#[derive(Serialize)]
struct InputNormativeOccurrence {
    file: String,
    line: usize,
    keyword: String,
}

#[derive(Serialize)]
struct InputUnregisteredDefinition {
    file: String,
    line: usize,
    id: String,
}

#[derive(Serialize)]
struct InputMalformedId {
    file: String,
    line: usize,
    id: String,
}

#[derive(Serialize)]
struct InputUnreachableReference {
    file: String,
    line: usize,
    dest: String,
    resolved: String,
}

/// A document-wide link, independent of any claim's C5 scope — the
/// register's document-level resolution surface
/// (`.ledger/2026-08-05-links-are-document-facts-not-claim-attributes.md`).
/// `dest` is the raw href, exactly like `InputClaim::prose_links`'
/// entries; register.ncl normalizes and resolves it the same way,
/// reusing `normalize_prose_link`/`resolves` rather than a second
/// mechanism.
#[derive(Serialize)]
struct InputDocumentLink {
    file: String,
    line: usize,
    dest: String,
}

#[derive(Serialize)]
struct InputStaleAbsenceMarker {
    file: String,
    line: usize,
    id: String,
    literal: String,
}

#[derive(Serialize)]
struct Input {
    claims: Vec<InputClaim>,
    documents: Vec<InputDocument>,
    genres: Vec<InputGenre>,
    orphan_claims: Vec<InputOrphanClaim>,
    normative_occurrences: Vec<InputNormativeOccurrence>,
    unregistered_definitions: Vec<InputUnregisteredDefinition>,
    malformed_ids: Vec<InputMalformedId>,
    unreachable_references: Vec<InputUnreachableReference>,
    links: Vec<InputDocumentLink>,
    stale_absence_markers: Vec<InputStaleAbsenceMarker>,
}

/// A YAML claim block re-parsed into JSON, for C1 — mirrors what the
/// per-claim contract call this replaces used to do per claim, done here
/// once while building the single payload instead.
fn parse_raw_yaml(yaml: &str) -> (Option<serde_json::Value>, Option<String>) {
    match serde_norway::from_str::<serde_norway::Value>(yaml) {
        Ok(value) => {
            let json = serde_json::to_value(&value)
                .expect("a parsed YAML value always re-serializes to JSON");
            (Some(json), None)
        }
        Err(e) => (None, Some(e.to_string())),
    }
}

fn build_input(loaded: &LoadedCorpus, config: &Config, markers: &[Marker]) -> Input {
    let corpus = &loaded.corpus;

    let claims = corpus
        .claims
        .iter()
        .map(|claim| {
            let (raw_value, raw_error) = parse_raw_yaml(&claim.raw.yaml);
            InputClaim {
                id: claim.id.clone(),
                file: claim.file.clone(),
                heading_line: claim.heading_line.0,
                block_line: claim.block_line.0,
                raw_value,
                raw_error,
                depends: claim.depends.iter().map(|c| c.to_string()).collect(),
                because: claim.because.iter().map(|c| c.to_string()).collect(),
                prose_links: claim.prose_links.clone(),
            }
        })
        .collect();

    let documents = corpus
        .documents
        .iter()
        .map(|doc| InputDocument {
            doc_path: doc.doc_path.clone(),
            file: doc.file.clone(),
            genre_path: doc.genre_path.clone(),
            headings: doc
                .headings
                .iter()
                .map(|h| InputHeading {
                    level: h.level,
                    text: h.text.clone(),
                    line: h.line.0,
                })
                .collect(),
        })
        .collect();

    let genres = config
        .genres
        .iter()
        .map(|g| InputGenre {
            path: g.path.clone(),
            kinds: g.kinds.iter().map(|k| k.as_str().to_string()).collect(),
        })
        .collect();

    let orphan_claims = loaded
        .orphan_claims
        .iter()
        .map(|o| InputOrphanClaim {
            file: o.file.clone(),
            line: o.line.0,
        })
        .collect();

    let normative_occurrences = loaded
        .normative_occurrences
        .iter()
        .map(|o| InputNormativeOccurrence {
            file: o.file.clone(),
            line: o.line.0,
            keyword: o.keyword.to_string(),
        })
        .collect();

    let unregistered_definitions = loaded
        .unregistered_definitions
        .iter()
        .map(|d| InputUnregisteredDefinition {
            file: d.file.clone(),
            line: d.line.0,
            id: d.id.clone(),
        })
        .collect();

    let malformed_ids = loaded
        .malformed_ids
        .iter()
        .map(|m| InputMalformedId {
            file: m.file.clone(),
            line: m.line.0,
            id: m.id.clone(),
        })
        .collect();

    let unreachable_references = loaded
        .unreachable_references
        .iter()
        .map(|u| InputUnreachableReference {
            file: u.file.clone(),
            line: u.line.0,
            dest: u.dest.clone(),
            resolved: u.resolved.clone(),
        })
        .collect();

    let links = loaded
        .links
        .iter()
        .map(|l| InputDocumentLink {
            file: l.file.clone(),
            line: l.line.0,
            dest: l.dest.clone(),
        })
        .collect();

    // `absent-marker-stale` (absence.rs): impure the same way
    // `unreachable_references` is — it needs the marker scan, a second
    // corpus-tree walk `load_corpus` never performs on its own — so it
    // is computed here, beside the rest of this module's I/O, rather
    // than downstream in register.ncl.
    let stale_absence_markers = absence::find_stale_markers(corpus, markers)
        .into_iter()
        .map(|s| InputStaleAbsenceMarker {
            file: s.file,
            line: s.line.0,
            id: s.id,
            literal: s.literal,
        })
        .collect();

    Input {
        claims,
        documents,
        genres,
        orphan_claims,
        normative_occurrences,
        unregistered_definitions,
        malformed_ids,
        unreachable_references,
        links,
        stale_absence_markers,
    }
}

// --- output: register.ncl's `{ index, diagnostics }` ---------------------

#[derive(serde::Deserialize)]
struct OutputIndexClaim {
    id: String,
    file: String,
    line: usize,
    kind: String,
    evaluator: String,
    depends: Vec<String>,
    because: Vec<String>,
}

#[derive(serde::Deserialize)]
struct OutputIndexDocument {
    doc_path: String,
    file: String,
    genre: String,
}

#[derive(serde::Deserialize)]
struct OutputIndex {
    claims: Vec<OutputIndexClaim>,
    documents: Vec<OutputIndexDocument>,
}

#[derive(serde::Deserialize)]
struct OutputDiagnostic {
    check: String,
    severity: String,
    file: String,
    line: usize,
    message: String,
}

#[derive(serde::Deserialize)]
struct Output {
    index: OutputIndex,
    diagnostics: Vec<OutputDiagnostic>,
}

/// Run the register evaluator over an already-loaded corpus.
/// `register_path` is an already-resolved `register.ncl` — the CLI
/// resolves it (docket's own embedded copy by default, `--register` to
/// override) before calling in; this function has no opinion on where it
/// came from. `markers` is the corpus's `@docket:` marker scan
/// (`marker::scan_markers`) — needed for `absent-marker-stale`
/// (absence.rs), the one diagnostic here that isn't derivable from the
/// document/config input alone.
pub fn run_checks(
    loaded: &LoadedCorpus,
    config: &Config,
    register_path: &Path,
    markers: &[Marker],
) -> Result<RegisterResult, RegisterError> {
    let input = build_input(loaded, config, markers);
    let input_json = serde_json::to_string(&input).expect("Input serializes");
    let value = nickel::evaluate_register(register_path, &input_json)?;
    let output: Output = serde_json::from_value(value)
        .expect("register.ncl guarantees the { index, diagnostics } output shape");

    let mut index = Index::default();
    for c in output.index.claims {
        index.claims.insert(
            c.id.clone(),
            IndexClaim {
                file: c.file,
                line: c.line,
                kind: c.kind,
                evaluator: c.evaluator,
                depends: c.depends,
                because: c.because,
            },
        );
    }
    for d in output.index.documents {
        index.documents.insert(
            d.doc_path.clone(),
            IndexDocument {
                file: d.file,
                genre: d.genre,
            },
        );
    }

    let diagnostics = output
        .diagnostics
        .into_iter()
        .map(|d| Diagnostic {
            check: d.check,
            severity: match d.severity.as_str() {
                "fail" => Severity::Fail,
                "warn" => Severity::Warn,
                other => panic!(
                    "register.ncl guarantees severity is \"fail\" or \"warn\", got {other:?}"
                ),
            },
            file: d.file,
            line: d.line,
            message: d.message,
        })
        .collect();

    Ok(RegisterResult {
        index,
        report: CheckReport { diagnostics },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::load_config;
    use crate::corpus::load_corpus;
    use std::io::Write;

    struct TempDir(std::path::PathBuf);
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
    impl TempDir {
        /// `git init`, for the one test below that needs a real
        /// repository to check `unreachable-reference` against — every
        /// other fixture in this module deliberately isn't a git
        /// repository, exercising the degrade-to-silence path by default.
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
            "docket-checks-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }

    /// The real project `contracts/register.ncl`, resolved the same way
    /// `config::load_config` resolves `contracts/docket.ncl` — relative
    /// to the process's current directory, which `cargo test` sets to
    /// the crate root. Unlike the per-claim contract this replaces,
    /// there is no minimal test double to substitute: the checks under
    /// test *are* register.ncl's logic, so exercising anything smaller
    /// would test something other than the real behaviour.
    fn register_path() -> std::path::PathBuf {
        std::path::PathBuf::from(DEFAULT_REGISTER_RELATIVE_PATH)
    }

    fn run(dir: &TempDir) -> CheckReport {
        let config = load_config(dir.path()).unwrap();
        let loaded = load_corpus(dir.path(), &config).unwrap();
        let markers = crate::marker::scan_markers(dir.path()).unwrap();
        run_checks(&loaded, &config, &register_path(), &markers)
            .unwrap()
            .report
    }

    fn only<'a>(report: &'a CheckReport, check: &str) -> Vec<&'a Diagnostic> {
        report
            .diagnostics
            .iter()
            .filter(|f| f.check == check)
            .collect()
    }

    #[test]
    fn a_clean_corpus_passes_every_check() {
        let dir = tempdir();
        dir.write(
            "docket.ncl",
            r#"{ genres = [ { path = "docs/specs/**", kinds = ["constraint"], quadrant = "reference" } ] }"#,
        );
        dir.write(
            "docs/specs/lock.md",
            "### [lock-groundness]\n\nEvery lock value MUST be ground.\n\n```claim\nkind: constraint\nevaluator: test\n```\n",
        );
        let report = run(&dir);
        assert!(report.passed(), "{:#?}", report.diagnostics);
    }

    #[test]
    fn c1_fails_on_an_unknown_field() {
        let dir = tempdir();
        dir.write(
            "docket.ncl",
            r#"{ genres = [ { path = "docs/specs/**", kinds = ["constraint"], quadrant = "reference" } ] }"#,
        );
        dir.write(
            "docs/specs/x.md",
            "### [x]\n\n```claim\nkind: constraint\nevaluator: test\ntypo: oops\n```\n",
        );
        let report = run(&dir);
        assert_eq!(only(&report, "C1").len(), 1);
    }

    #[test]
    fn c1_reports_every_malformed_claim_in_one_evaluation() {
        // The property the C1 collapse must not lose: a contract applied
        // to an array aborts at the first violating element (verified
        // directly against Nickel 1.17.0), so register.ncl's C1 is a
        // hand-rolled batch validator instead. Two malformed claims, two
        // C1 diagnostics.
        let dir = tempdir();
        dir.write(
            "docket.ncl",
            r#"{ genres = [ { path = "docs/specs/**", kinds = ["constraint"], quadrant = "reference" } ] }"#,
        );
        dir.write(
            "docs/specs/a.md",
            "### [a]\n\n```claim\nkind: constraint\nevaluator: test\ntypo: oops\n```\n",
        );
        dir.write(
            "docs/specs/b.md",
            "### [b]\n\n```claim\nkind: constraint\nevaluator: vibes\n```\n",
        );
        let report = run(&dir);
        assert_eq!(only(&report, "C1").len(), 2, "{:#?}", report.diagnostics);
    }

    #[test]
    fn c1_fails_on_yaml_that_does_not_even_parse() {
        // A YAML document that doesn't parse at all is itself a C1
        // violation (MVP.md §3: "malformed or unknown field"), reported
        // the same way a contract-rejected value is.
        let dir = tempdir();
        dir.write(
            "docket.ncl",
            r#"{ genres = [ { path = "docs/specs/**", kinds = ["constraint"], quadrant = "reference" } ] }"#,
        );
        dir.write(
            "docs/specs/x.md",
            "### [x]\n\n```claim\nkind: [unterminated\n```\n",
        );
        let report = run(&dir);
        let failures = only(&report, "C1");
        assert_eq!(failures.len(), 1, "{:#?}", report.diagnostics);
        assert!(
            failures[0].message.contains("invalid YAML"),
            "{:?}",
            failures[0].message
        );
    }

    #[test]
    fn c2_fails_on_duplicate_ids() {
        let dir = tempdir();
        dir.write(
            "docket.ncl",
            r#"{ genres = [ { path = "docs/specs/**", kinds = ["constraint"], quadrant = "reference" } ] }"#,
        );
        dir.write(
            "docs/specs/a.md",
            "### [dup]\n\n```claim\nkind: constraint\nevaluator: test\n```\n",
        );
        dir.write(
            "docs/specs/b.md",
            "### [dup]\n\n```claim\nkind: constraint\nevaluator: test\n```\n",
        );
        let report = run(&dir);
        assert_eq!(only(&report, "C2").len(), 2);
    }

    #[test]
    fn c3_fails_when_kind_is_not_permitted_by_genre() {
        let dir = tempdir();
        dir.write(
            "docket.ncl",
            r#"{ genres = [ { path = "docs/adr/**", kinds = [], quadrant = "explanation" } ] }"#,
        );
        dir.write(
            "docs/adr/decision.md",
            "### [normative-leak]\n\n```claim\nkind: constraint\nevaluator: test\n```\n",
        );
        let report = run(&dir);
        assert_eq!(only(&report, "C3").len(), 1);
    }

    #[test]
    fn c3_passes_when_kind_is_permitted() {
        let dir = tempdir();
        dir.write(
            "docket.ncl",
            r#"{ genres = [ { path = "docs/models/**", kinds = ["invariant"], quadrant = "reference" } ] }"#,
        );
        dir.write(
            "docs/models/m.md",
            "### [ok]\n\n```claim\nkind: invariant\nevaluator: test\n```\n",
        );
        let report = run(&dir);
        assert!(only(&report, "C3").is_empty());
    }

    #[test]
    fn c4_fails_on_a_dangling_claim_id_depends() {
        let dir = tempdir();
        dir.write(
            "docket.ncl",
            r#"{ genres = [ { path = "docs/specs/**", kinds = ["constraint"], quadrant = "reference" } ] }"#,
        );
        dir.write(
            "docs/specs/x.md",
            "### [x]\n\n```claim\nkind: constraint\nevaluator: test\ndepends: [does-not-exist]\n```\n",
        );
        let report = run(&dir);
        let failures = only(&report, "C4");
        assert_eq!(failures.len(), 1);
        // Criterion 2: the message says the claim is broken, not merely
        // "does not resolve" — the severity split (R1) is only real if a
        // reader can tell the two remedies apart from the text alone.
        assert!(
            failures[0].message.contains("broken"),
            "{:?}",
            failures[0].message
        );
        assert_eq!(failures[0].severity, Severity::Fail);
        assert!(!report.passed());
    }

    #[test]
    fn a_dangling_depends_with_a_matching_prose_link_fails_only_c4_not_c5() {
        // The exact case MVP.md §3's boxed note calls out: prose and
        // depends AGREE (both name the same nonexistent target), so only
        // C4 ("this target does not exist") should fire — a resolution
        // filter on C5's L would incorrectly also fire C5 here, with a
        // diagnostic that lies about a divergence that doesn't exist.
        let dir = tempdir();
        dir.write(
            "docket.ncl",
            r#"{ genres = [ { path = "docs/specs/**", kinds = ["constraint"], quadrant = "reference" } ] }"#,
        );
        dir.write(
            "docs/specs/x.md",
            "### [x]\n\nSee [related work](does-not-exist).\n\n```claim\nkind: constraint\nevaluator: test\ndepends: [does-not-exist]\n```\n",
        );
        let report = run(&dir);
        assert_eq!(only(&report, "C4").len(), 1);
        assert!(only(&report, "C5").is_empty(), "{:#?}", report.diagnostics);
        // The same target, read document-wide, must not ALSO surface as a
        // separate `dangling-reference` — C4 already owns "this claim is
        // broken" for a declared depends/because target, at its own
        // severity; reporting the identical broken target a second time
        // under a different diagnosis would be noise, not a second finding.
        assert!(
            only(&report, "dangling-reference").is_empty(),
            "{:#?}",
            report.diagnostics
        );
    }

    #[test]
    fn orphaned_because_fails_with_warn_severity_and_a_distinct_message() {
        // Criterion 3: a dangling `because` target is a DISTINCT,
        // lower-severity result from C4 — the claim stands, only its
        // stated reason is orphaned. `report.passed()` must stay true: a
        // `Warn`-only report never flips MVP.md §5's exit code.
        let dir = tempdir();
        dir.write(
            "docket.ncl",
            r#"{ genres = [ { path = "docs/specs/**", kinds = ["constraint"], quadrant = "reference" } ] }"#,
        );
        dir.write(
            "docs/specs/x.md",
            "### [x]\n\nSee [the old reason](does-not-exist).\n\n```claim\nkind: constraint\nevaluator: test\nbecause: [does-not-exist]\n```\n",
        );
        let report = run(&dir);
        let warnings = only(&report, "orphaned-because");
        assert_eq!(warnings.len(), 1, "{:#?}", report.diagnostics);
        assert_eq!(warnings[0].severity, Severity::Warn);
        assert!(
            warnings[0].message.contains("orphaned"),
            "{:?}",
            warnings[0].message
        );
        assert!(
            !warnings[0].message.contains("broken"),
            "must not use C4's wording — the claim itself still stands: {:?}",
            warnings[0].message
        );
        assert!(only(&report, "C4").is_empty());
        assert!(
            report.passed(),
            "a Warn-only report must still pass: {:#?}",
            report.diagnostics
        );
    }

    #[test]
    fn a_bare_reference_to_a_nonexistent_target_produces_no_failure() {
        // Criterion 4, the noise-suppression property (R1's core reason
        // for the third kind): a prose link that is declared as neither
        // `depends` nor `because` is bare — it asserts no dependence, so
        // its target not resolving does not break anything. No C4, no
        // orphaned-because, no C5 (the new C5 only requires declared refs
        // to have a prose link, never the reverse) — and `report.passed()`
        // stays true, since a `Warn` never flips the exit code.
        //
        // It is no longer SILENT, though
        // (`.ledger/2026-08-05-links-are-document-facts-not-claim-attributes.md`):
        // "bare asserts nothing" is a claim about severity, not about
        // visibility — a dangling bare reference is now a `dangling-reference`
        // finding, which is what changed here.
        let dir = tempdir();
        dir.write(
            "docket.ncl",
            r#"{ genres = [ { path = "docs/specs/**", kinds = ["constraint"], quadrant = "reference" } ] }"#,
        );
        dir.write(
            "docs/specs/x.md",
            "### [x]\n\nSee also [related, unrelated work](does-not-exist), mentioned in passing.\n\n```claim\nkind: constraint\nevaluator: test\n```\n",
        );
        let report = run(&dir);
        assert!(only(&report, "C4").is_empty(), "{:#?}", report.diagnostics);
        assert!(only(&report, "C5").is_empty(), "{:#?}", report.diagnostics);
        assert!(
            only(&report, "orphaned-because").is_empty(),
            "{:#?}",
            report.diagnostics
        );
        let findings = only(&report, "dangling-reference");
        assert_eq!(findings.len(), 1, "{:#?}", report.diagnostics);
        assert_eq!(findings[0].severity, Severity::Warn);
        assert!(
            report.passed(),
            "a bare reference must never fail the report: {:#?}",
            report.diagnostics
        );
    }

    #[test]
    fn c4_resolves_a_numbered_document_anchor() {
        let dir = tempdir();
        dir.write(
            "docket.ncl",
            r#"{
              genres = [
                { path = "docs/specs/**", kinds = ["constraint"], quadrant = "reference" },
                { path = "docs/models/**", kinds = ["invariant"], quadrant = "reference" },
              ],
            }"#,
        );
        dir.write(
            "docs/models/composition-model.md",
            "## 6. The fact-set: the substrate's only state\n\nprose\n",
        );
        dir.write(
            "docs/specs/x.md",
            "### [x]\n\n```claim\nkind: constraint\nevaluator: test\ndepends: [docs/models/composition-model#6]\n```\n",
        );
        let report = run(&dir);
        assert!(only(&report, "C4").is_empty(), "{:#?}", report.diagnostics);
    }

    #[test]
    fn c4_rejects_a_numeric_prefix_collision() {
        let dir = tempdir();
        dir.write(
            "docket.ncl",
            r#"{
              genres = [
                { path = "docs/specs/**", kinds = ["constraint"], quadrant = "reference" },
                { path = "docs/models/**", kinds = ["invariant"], quadrant = "reference" },
              ],
            }"#,
        );
        dir.write("docs/models/m.md", "## 60. Something else\n");
        dir.write(
            "docs/specs/x.md",
            "### [x]\n\n```claim\nkind: constraint\nevaluator: test\ndepends: [docs/models/m#6]\n```\n",
        );
        let report = run(&dir);
        assert_eq!(only(&report, "C4").len(), 1);
    }

    #[test]
    fn c4_two_documents_sharing_a_basename_resolve_independently() {
        // The exact real-corpus shape that retired duplicate-stem: three
        // README.md files under one genre, indistinguishable by
        // basename. Both must resolve correctly by their own path.
        let dir = tempdir();
        dir.write(
            "docket.ncl",
            r#"{
              genres = [
                { path = "docs/specs/**", kinds = ["constraint"], quadrant = "reference" },
                { path = "docs/models/**", kinds = ["invariant"], quadrant = "reference" },
              ],
            }"#,
        );
        dir.write("docs/models/lean/README.md", "## 1. Lean notes\n");
        dir.write("docs/models/tla/README.md", "## 1. TLA notes\n");
        dir.write(
            "docs/specs/x.md",
            "### [x]\n\n```claim\nkind: constraint\nevaluator: test\ndepends: [docs/models/lean/README#1, docs/models/tla/README#1]\n```\n",
        );
        let report = run(&dir);
        assert!(only(&report, "C4").is_empty(), "{:#?}", report.diagnostics);
    }

    #[test]
    fn c5_passes_when_prose_link_covers_a_depends_entry() {
        let dir = tempdir();
        dir.write(
            "docket.ncl",
            r#"{
              genres = [
                { path = "docs/specs/**", kinds = ["constraint"], quadrant = "reference" },
                { path = "docs/models/**", kinds = ["invariant"], quadrant = "reference" },
              ],
            }"#,
        );
        dir.write("docs/models/composition-model.md", "## 6. The fact-set\n");
        dir.write(
            "docs/specs/x.md",
            "### [x]\n\nSee [the fact-set](../models/composition-model.md#6).\n\n```claim\nkind: constraint\nevaluator: test\ndepends: [docs/models/composition-model#6]\n```\n",
        );
        let report = run(&dir);
        assert!(only(&report, "C5").is_empty(), "{:#?}", report.diagnostics);
    }

    #[test]
    fn c5_passes_when_prose_link_covers_a_because_entry() {
        // R3's rule reads "every `depends` AND `because` entry carries a
        // prose link" — this proves C5 actually enforces the `because`
        // half too, not only `depends`.
        let dir = tempdir();
        dir.write(
            "docket.ncl",
            r#"{
              genres = [
                { path = "docs/specs/**", kinds = ["constraint"], quadrant = "reference" },
                { path = "docs/models/**", kinds = ["invariant"], quadrant = "reference" },
              ],
            }"#,
        );
        dir.write("docs/models/composition-model.md", "## 6. The fact-set\n");
        dir.write(
            "docs/specs/x.md",
            "### [x]\n\nSee [the fact-set](../models/composition-model.md#6).\n\n```claim\nkind: constraint\nevaluator: test\nbecause: [docs/models/composition-model#6]\n```\n",
        );
        let report = run(&dir);
        assert!(only(&report, "C5").is_empty(), "{:#?}", report.diagnostics);
    }

    #[test]
    fn c5_fails_when_a_depends_entry_has_no_prose_link() {
        // Criterion 5, first half: a `depends` entry with no matching
        // prose link fails C5's replacement rule.
        let dir = tempdir();
        dir.write(
            "docket.ncl",
            r#"{
              genres = [
                { path = "docs/specs/**", kinds = ["constraint"], quadrant = "reference" },
                { path = "docs/models/**", kinds = ["invariant"], quadrant = "reference" },
              ],
            }"#,
        );
        dir.write("docs/models/composition-model.md", "## 6. The fact-set\n");
        dir.write(
            "docs/specs/x.md",
            "### [x]\n\nNo links here.\n\n```claim\nkind: constraint\nevaluator: test\ndepends: [docs/models/composition-model#6]\n```\n",
        );
        let report = run(&dir);
        let failures = only(&report, "C5");
        assert_eq!(failures.len(), 1);
        assert!(
            failures[0].message.contains("depends:"),
            "{:?}",
            failures[0].message
        );
    }

    #[test]
    fn c5_fails_when_a_because_entry_has_no_prose_link() {
        let dir = tempdir();
        dir.write(
            "docket.ncl",
            r#"{
              genres = [
                { path = "docs/specs/**", kinds = ["constraint"], quadrant = "reference" },
                { path = "docs/models/**", kinds = ["invariant"], quadrant = "reference" },
              ],
            }"#,
        );
        dir.write("docs/models/composition-model.md", "## 6. The fact-set\n");
        dir.write(
            "docs/specs/x.md",
            "### [x]\n\nNo links here.\n\n```claim\nkind: constraint\nevaluator: test\nbecause: [docs/models/composition-model#6]\n```\n",
        );
        let report = run(&dir);
        let failures = only(&report, "C5");
        assert_eq!(failures.len(), 1);
        assert!(
            failures[0].message.contains("because:"),
            "{:?}",
            failures[0].message
        );
    }

    #[test]
    fn c5_does_not_require_an_undeclared_prose_link_to_be_declared() {
        // Criterion 5, second half: a prose link with no declaration does
        // NOT fail C5 — it is a bare reference (R3), and C5's new rule is
        // one-directional (declared ⊆ prose), not the old set equality.
        let dir = tempdir();
        dir.write(
            "docket.ncl",
            r#"{ genres = [ { path = "docs/specs/**", kinds = ["constraint"], quadrant = "reference" } ] }"#,
        );
        dir.write(
            "docs/specs/x.md",
            "### [x]\n\nSee [context, not a dependency](https://example.com/context).\n\n```claim\nkind: constraint\nevaluator: test\n```\n",
        );
        let report = run(&dir);
        assert!(only(&report, "C5").is_empty(), "{:#?}", report.diagnostics);
    }

    #[test]
    fn c5_ignores_an_external_url_and_a_link_outside_the_corpus() {
        let dir = tempdir();
        dir.write(
            "docket.ncl",
            r#"{ genres = [ { path = "docs/specs/**", kinds = ["constraint"], quadrant = "reference" } ] }"#,
        );
        dir.write(
            "docs/specs/x.md",
            "### [x]\n\nSee [the web](https://example.com) and [escaping the root](../../outside.md).\n\n```claim\nkind: constraint\nevaluator: test\n```\n",
        );
        let report = run(&dir);
        assert!(only(&report, "C5").is_empty(), "{:#?}", report.diagnostics);
    }

    #[test]
    fn c5_accepts_an_anchor_form_prose_link_for_a_claim_id_declaration() {
        // .ledger/2026-07-30-claim-id-prose-link-breaks-link-checkers.md:
        // the anchor form (`#id`) is what an ordinary link checker and an
        // author both understand for a same-file link. A claim-id
        // `depends` entry must be satisfiable by it, not only by the bare
        // form (`[…](target-claim)`, unlinkable outside this tool).
        let dir = tempdir();
        dir.write(
            "docket.ncl",
            r#"{ genres = [ { path = "docs/specs/**", kinds = ["constraint"], quadrant = "reference" } ] }"#,
        );
        dir.write(
            "docs/specs/a.md",
            "### [target-claim]\n\n```claim\nkind: constraint\nevaluator: test\n```\n\n\
             ### [depends-on-target]\n\nSee [the target](#target-claim).\n\n\
             ```claim\nkind: constraint\nevaluator: test\ndepends: [target-claim]\n```\n",
        );
        let report = run(&dir);
        assert!(only(&report, "C5").is_empty(), "{:#?}", report.diagnostics);
    }

    #[test]
    fn c5_accepts_a_cross_file_anchor_form_prose_link_for_a_claim_id_declaration() {
        // The anchor doesn't have to be same-file: a doc-anchor prose
        // link whose *anchor component* is the claim id satisfies the
        // declaration regardless of which document it points at.
        let dir = tempdir();
        dir.write(
            "docket.ncl",
            r#"{
              genres = [
                { path = "docs/specs/**", kinds = ["constraint"], quadrant = "reference" },
                { path = "docs/models/**", kinds = ["invariant"], quadrant = "reference" },
              ],
            }"#,
        );
        dir.write(
            "docs/specs/a.md",
            "### [target-claim]\n\n```claim\nkind: constraint\nevaluator: test\n```\n",
        );
        dir.write(
            "docs/models/b.md",
            "### [depends-on-target]\n\nSee [the target](../specs/a.md#target-claim).\n\n\
             ```claim\nkind: invariant\nevaluator: test\ndepends: [target-claim]\n```\n",
        );
        let report = run(&dir);
        assert!(only(&report, "C5").is_empty(), "{:#?}", report.diagnostics);
    }

    #[test]
    fn c5_still_fails_a_claim_id_depends_entry_with_no_prose_link_at_all() {
        // The property most easily lost by this fix: accepting the
        // anchor form must not turn into accepting *no* link. A
        // claim-id `depends` entry with zero prose links anywhere in the
        // claim's body still fails C5.
        let dir = tempdir();
        dir.write(
            "docket.ncl",
            r#"{ genres = [ { path = "docs/specs/**", kinds = ["constraint"], quadrant = "reference" } ] }"#,
        );
        dir.write(
            "docs/specs/a.md",
            "### [target-claim]\n\n```claim\nkind: constraint\nevaluator: test\n```\n\n\
             ### [depends-on-target]\n\nNo links here.\n\n\
             ```claim\nkind: constraint\nevaluator: test\ndepends: [target-claim]\n```\n",
        );
        let report = run(&dir);
        let failures = only(&report, "C5");
        assert_eq!(failures.len(), 1, "{:#?}", report.diagnostics);
        assert!(
            failures[0].message.contains("depends:target-claim"),
            "{:?}",
            failures[0].message
        );
    }

    // --- normalize_prose_link edge cases, now internal to register.ncl --
    // (formerly direct Rust unit tests on `checks::normalize_prose_link`;
    // that function has no Rust form to unit-test anymore, so its shape
    // rules are pinned here through C4/C5's observable behaviour instead.)

    #[test]
    fn c5_resolves_a_bare_sibling_token_with_an_anchor_in_the_same_directory() {
        // "sibling#3": no `/`, but an anchor is present, so this is a
        // document-anchor reference resolved against the citing file's
        // own directory, not a claim id.
        let dir = tempdir();
        dir.write(
            "docket.ncl",
            r#"{ genres = [ { path = "docs/specs/**", kinds = ["constraint"], quadrant = "reference" } ] }"#,
        );
        dir.write("docs/specs/sibling.md", "## 3. Something\n");
        dir.write(
            "docs/specs/x.md",
            "### [x]\n\nSee [nearby](sibling#3).\n\n```claim\nkind: constraint\nevaluator: test\ndepends: [docs/specs/sibling#3]\n```\n",
        );
        let report = run(&dir);
        assert!(only(&report, "C4").is_empty(), "{:#?}", report.diagnostics);
        assert!(only(&report, "C5").is_empty(), "{:#?}", report.diagnostics);
    }

    #[test]
    fn c5_does_not_accept_a_whole_document_link_with_no_anchor() {
        // A path with no anchor has no representation in the ref syntax
        // depends/because share, so it never joins C5's `L` — a
        // `depends` entry naming an anchor still fails C5 even though
        // the prose links the same document.
        let dir = tempdir();
        dir.write(
            "docket.ncl",
            r#"{ genres = [ { path = "docs/specs/**", kinds = ["constraint"], quadrant = "reference" } ] }"#,
        );
        dir.write("docs/specs/other.md", "## 1. Heading\n");
        dir.write(
            "docs/specs/x.md",
            "### [x]\n\nSee [the whole doc](other.md).\n\n```claim\nkind: constraint\nevaluator: test\ndepends: [docs/specs/other#1]\n```\n",
        );
        let report = run(&dir);
        assert!(
            only(&report, "C4").is_empty(),
            "the depends target itself still resolves: {:#?}",
            report.diagnostics
        );
        assert_eq!(only(&report, "C5").len(), 1, "{:#?}", report.diagnostics);
    }

    #[test]
    fn c5_does_not_accept_a_prose_link_that_escapes_the_corpus_root() {
        // A `..` that would pop past the corpus root drops the link
        // entirely (MVP.md §1.3) — it must not count toward C5's `L`
        // even when its anchor happens to match a real depends target.
        let dir = tempdir();
        dir.write(
            "docket.ncl",
            r#"{ genres = [ { path = "docs/specs/**", kinds = ["constraint"], quadrant = "reference" } ] }"#,
        );
        dir.write("docs/specs/target.md", "## 1. Heading\n");
        dir.write(
            "docs/specs/x.md",
            "### [x]\n\nSee [nope](../../../outside.md#1).\n\n```claim\nkind: constraint\nevaluator: test\ndepends: [docs/specs/target#1]\n```\n",
        );
        let report = run(&dir);
        assert!(only(&report, "C4").is_empty(), "{:#?}", report.diagnostics);
        assert_eq!(only(&report, "C5").len(), 1, "{:#?}", report.diagnostics);
    }

    #[test]
    fn c5_accepts_a_leading_slash_link_as_corpus_root_relative() {
        // Not addressed by MVP.md's worked example — this project's own
        // judgment call: a leading `/` is corpus-root-relative rather
        // than joined onto the citing file's own directory.
        let dir = tempdir();
        dir.write(
            "docket.ncl",
            r#"{
              genres = [
                { path = "docs/specs/**", kinds = ["constraint"], quadrant = "reference" },
                { path = "docs/models/**", kinds = ["invariant"], quadrant = "reference" },
              ],
            }"#,
        );
        dir.write("docs/models/composition-model.md", "## 6. The fact-set\n");
        dir.write(
            "docs/specs/deep/nested/x.md",
            "### [x]\n\nSee [cm](/docs/models/composition-model.md#6).\n\n```claim\nkind: constraint\nevaluator: test\ndepends: [docs/models/composition-model#6]\n```\n",
        );
        let report = run(&dir);
        assert!(only(&report, "C4").is_empty(), "{:#?}", report.diagnostics);
        assert!(only(&report, "C5").is_empty(), "{:#?}", report.diagnostics);
    }

    // --- normative-prose ---------------------------------------------

    #[test]
    fn normative_prose_fails_on_a_bare_keyword_in_a_kinds_empty_genre() {
        // Criterion 2: a `kinds = []` genre holding a bare MUST in its
        // own voice, no claim block anywhere in the file.
        let dir = tempdir();
        dir.write(
            "docket.ncl",
            r#"{ genres = [ { path = "docs/adr/**", kinds = [], quadrant = "explanation" } ] }"#,
        );
        dir.write(
            "docs/adr/0001-decision.md",
            "# ADR 0001: decision\n\nThis decision MUST be treated as final.\n",
        );
        let report = run(&dir);
        assert_eq!(
            only(&report, "normative-prose").len(),
            1,
            "{:#?}",
            report.diagnostics
        );
    }

    #[test]
    fn normative_prose_passes_when_the_keyword_is_only_quoted_or_coded() {
        // Criterion 3: the same keyword, present only inside a block
        // quote and inside an inline code span — the check must PASS,
        // proving the design (a quote/code exemption) rather than a grep.
        let dir = tempdir();
        dir.write(
            "docket.ncl",
            r#"{ genres = [ { path = "docs/adr/**", kinds = [], quadrant = "explanation" } ] }"#,
        );
        dir.write(
            "docs/adr/0001-decision.md",
            "# ADR 0001: decision\n\n> The rejected proposal said the service MUST retry.\n\nWe reject that. Note `MUST` above is quoted, not asserted.\n",
        );
        let report = run(&dir);
        assert!(
            only(&report, "normative-prose").is_empty(),
            "{:#?}",
            report.diagnostics
        );
    }

    #[test]
    fn normative_prose_is_silent_in_a_genre_that_permits_kinds() {
        // Criterion 4: a genre that permits kinds is unaffected — its job
        // is to say MUST.
        let dir = tempdir();
        dir.write(
            "docket.ncl",
            r#"{ genres = [ { path = "docs/specs/**", kinds = ["constraint"], quadrant = "reference" } ] }"#,
        );
        dir.write(
            "docs/specs/lock.md",
            "### [lock-groundness]\n\nEvery lock value MUST be ground.\n\n```claim\nkind: constraint\nevaluator: test\n```\n",
        );
        let report = run(&dir);
        assert!(
            only(&report, "normative-prose").is_empty(),
            "{:#?}",
            report.diagnostics
        );
    }

    #[test]
    fn normative_prose_names_the_file_line_and_genre() {
        let dir = tempdir();
        dir.write(
            "docket.ncl",
            r#"{ genres = [ { path = "docs/adr/**", kinds = [], quadrant = "explanation" } ] }"#,
        );
        dir.write(
            "docs/adr/x.md",
            "# ADR\n\nline2\n\nThis SHALL NOT be reopened.\n",
        );
        let report = run(&dir);
        let failures = only(&report, "normative-prose");
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].file, "docs/adr/x.md");
        assert_eq!(failures[0].line, 5);
        assert!(failures[0].message.contains("SHALL NOT"));
        assert!(failures[0].message.contains("docs/adr/**"));
    }

    #[test]
    fn orphan_claim_is_reported() {
        let dir = tempdir();
        dir.write(
            "docket.ncl",
            r#"{ genres = [ { path = "docs/specs/**", kinds = ["constraint"], quadrant = "reference" } ] }"#,
        );
        dir.write(
            "docs/specs/x.md",
            "## Notes\n\n```claim\nkind: constraint\n```\n",
        );
        let report = run(&dir);
        assert_eq!(only(&report, "orphan-claim").len(), 1);
    }

    // --- bold-form recognizer: C2 across recognizers, coverage count ------

    #[test]
    fn c2_fails_on_one_id_declared_via_both_heading_and_bold_form() {
        // Criterion 5: two definitions of one id, arriving via DIFFERENT
        // recognizers (heading-form in one file, bold-form in another),
        // must still be caught as a single C2 duplicate naming both
        // sites — C2 groups by claim id alone, so this needs no new
        // logic, only proof it actually holds.
        let dir = tempdir();
        dir.write(
            "docket.ncl",
            r#"{ genres = [ { path = "docs/specs/**", kinds = ["constraint"], quadrant = "reference" } ] }"#,
        );
        dir.write(
            "docs/specs/a.md",
            "### [dup-across-forms]\n\n```claim\nkind: constraint\nevaluator: test\n```\n",
        );
        dir.write(
            "docs/specs/b.md",
            "**[dup-across-forms]**: A second definition, bold-form this time.\n\n```claim\nkind: constraint\nevaluator: test\n```\n",
        );
        let report = run(&dir);
        let failures = only(&report, "C2");
        // One diagnostic per site (existing C2 shape), each naming the
        // OTHER site in its message — together the pair names both
        // locations, exactly what the dispatch's criterion 5 asks for.
        assert_eq!(failures.len(), 2, "{:#?}", report.diagnostics);
        let at_a = failures.iter().find(|d| d.file == "docs/specs/a.md");
        let at_b = failures.iter().find(|d| d.file == "docs/specs/b.md");
        assert!(at_a.is_some() && at_b.is_some(), "{:#?}", failures);
        assert!(at_a.unwrap().message.contains("docs/specs/b.md"));
        assert!(at_b.unwrap().message.contains("docs/specs/a.md"));
    }

    #[test]
    fn unregistered_definition_is_reported_but_never_fails_the_report() {
        let dir = tempdir();
        dir.write(
            "docket.ncl",
            r#"{ genres = [ { path = "docs/specs/**", kinds = ["constraint"], quadrant = "reference" } ] }"#,
        );
        dir.write(
            "docs/specs/x.md",
            "**[not-yet-registered]**: A real definition with no block yet.\n",
        );
        let report = run(&dir);
        let warnings = only(&report, "unregistered-definition");
        assert_eq!(warnings.len(), 1, "{:#?}", report.diagnostics);
        assert_eq!(warnings[0].severity, Severity::Warn);
        assert!(warnings[0].message.contains("not-yet-registered"));
        assert!(
            report.passed(),
            "unregistered-definition must never flip the exit code: {:#?}",
            report.diagnostics
        );
    }

    #[test]
    fn malformed_id_is_reported_at_warn_severity_and_never_fails_the_report() {
        // `.ledger/2026-08-04-malformed-ids-are-silently-invisible.md`'s
        // exact real-corpus shape: an otherwise-kebab id with one stray
        // uppercase segment, at Warn severity like `unregistered-definition`
        // — advisory, never blocking a commit already in flight.
        let dir = tempdir();
        dir.write(
            "docket.ncl",
            r#"{ genres = [ { path = "docs/specs/**", kinds = ["constraint"], quadrant = "reference" } ] }"#,
        );
        dir.write(
            "docs/specs/x.md",
            "**[boundary-L1-concerns]**: L1 (atom) owns content addressing.\n",
        );
        let report = run(&dir);
        let warnings = only(&report, "malformed-id");
        assert_eq!(warnings.len(), 1, "{:#?}", report.diagnostics);
        assert_eq!(warnings[0].severity, Severity::Warn);
        assert!(warnings[0].message.contains("boundary-L1-concerns"));
        assert!(
            report.passed(),
            "malformed-id must never flip the exit code: {:#?}",
            report.diagnostics
        );
    }

    #[test]
    fn malformed_id_is_distinct_from_unregistered_definition() {
        // The remedies differ (rename vs. write a claim block), so a
        // malformed id must never also surface as
        // `unregistered-definition` — it was never a recognized
        // definition at all.
        let dir = tempdir();
        dir.write(
            "docket.ncl",
            r#"{ genres = [ { path = "docs/specs/**", kinds = ["constraint"], quadrant = "reference" } ] }"#,
        );
        dir.write(
            "docs/specs/x.md",
            "**[daemon-discovery-vN]**: In future versions, ion MAY support\nadditional discovery mechanisms.\n",
        );
        let report = run(&dir);
        assert_eq!(
            only(&report, "malformed-id").len(),
            1,
            "{:#?}",
            report.diagnostics
        );
        assert!(
            only(&report, "unregistered-definition").is_empty(),
            "{:#?}",
            report.diagnostics
        );
    }

    #[test]
    fn a_multi_word_bracket_never_fires_malformed_id_through_the_register() {
        // The false-positive floor, exercised end-to-end through the real
        // register.ncl rather than only at extract.rs's unit level.
        let dir = tempdir();
        dir.write(
            "docket.ncl",
            r#"{ genres = [ { path = "docs/specs/**", kinds = ["constraint"], quadrant = "reference" } ] }"#,
        );
        dir.write(
            "docs/specs/x.md",
            "**[Note to reader]**: this is prose, not an id.\n",
        );
        let report = run(&dir);
        assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    }

    // --- unreachable-reference --------------------------------------------

    #[test]
    fn a_link_to_a_gitignored_path_fails_at_fail_severity() {
        // `.ledger/2026-08-05-references-that-leave-the-register.md`, O4:
        // the "unreachable" condition, exercised end-to-end through the
        // real register.ncl — gitignore.rs's own tests already cover the
        // git-query logic in isolation; this proves the fact it produces
        // actually reaches a Diagnostic in the shape MVP.md §3 promises.
        let dir = tempdir();
        dir.git_init();
        dir.write(".gitignore", ".scratch/\n");
        dir.write(
            "docket.ncl",
            r#"{ genres = [ { path = "docs/specs/**", kinds = ["constraint"], quadrant = "reference" } ] }"#,
        );
        dir.write(
            "docs/specs/x.md",
            "### [x]\n\nSee [notes](../../.scratch/notes.md).\n\n```claim\nkind: constraint\nevaluator: test\n```\n",
        );
        let report = run(&dir);
        let failures = only(&report, "unreachable-reference");
        assert_eq!(failures.len(), 1, "{:#?}", report.diagnostics);
        assert_eq!(failures[0].severity, Severity::Fail);
        assert_eq!(failures[0].file, "docs/specs/x.md");
        assert!(failures[0].message.contains(".scratch/notes.md"));
        assert!(!report.passed(), "{:#?}", report.diagnostics);
    }

    #[test]
    fn a_link_to_an_ordinary_tracked_path_does_not_fire_unreachable_reference() {
        let dir = tempdir();
        dir.git_init();
        dir.write(".gitignore", ".scratch/\n");
        dir.write(
            "docket.ncl",
            r#"{ genres = [ { path = "docs/specs/**", kinds = ["constraint"], quadrant = "reference" } ] }"#,
        );
        dir.write("docs/specs/y.md", "### [y]\n");
        dir.write(
            "docs/specs/x.md",
            "### [x]\n\nSee [y](y.md).\n\n```claim\nkind: constraint\nevaluator: test\n```\n",
        );
        let report = run(&dir);
        assert!(
            only(&report, "unreachable-reference").is_empty(),
            "{:#?}",
            report.diagnostics
        );
    }

    #[test]
    fn a_corpus_that_is_not_a_git_repository_never_fires_unreachable_reference() {
        // The degrade-to-silence case (dispatch): no git history to ask
        // means no reader-reachability question this check can answer —
        // never a crash, never a false verdict.
        let dir = tempdir();
        dir.write(
            "docket.ncl",
            r#"{ genres = [ { path = "docs/specs/**", kinds = ["constraint"], quadrant = "reference" } ] }"#,
        );
        dir.write(
            "docs/specs/x.md",
            "### [x]\n\nSee [notes](../../.scratch/notes.md).\n\n```claim\nkind: constraint\nevaluator: test\n```\n",
        );
        let report = run(&dir);
        assert!(
            only(&report, "unreachable-reference").is_empty(),
            "{:#?}",
            report.diagnostics
        );
    }

    #[test]
    fn a_bare_claim_id_shaped_link_is_never_checked_against_git_even_if_the_name_collides() {
        // Scope boundary: "target" with no slash/dot/anchor reads as a
        // claim-id candidate, not a path — out of scope even though a
        // directory named exactly that is really gitignored here.
        let dir = tempdir();
        dir.git_init();
        dir.write(".gitignore", "target/\n");
        dir.write(
            "docket.ncl",
            r#"{ genres = [ { path = "docs/specs/**", kinds = ["constraint"], quadrant = "reference" } ] }"#,
        );
        dir.write(
            "docs/specs/x.md",
            "### [target]\n\nSee [target](target).\n\n```claim\nkind: constraint\nevaluator: test\n```\n",
        );
        let report = run(&dir);
        assert!(
            only(&report, "unreachable-reference").is_empty(),
            "{:#?}",
            report.diagnostics
        );
    }

    #[test]
    fn a_reference_into_the_repository_from_outside_it_is_never_this_checks_concern() {
        // The directional half of the rule (dispatch): an ignored file
        // linking INTO the repository is fine — "only the author has
        // that." Structural, not a filter: `load_corpus` never walks a
        // dotdir at all, so an ignored document's own links can never
        // reach this check's input to begin with. Proven here by scanning
        // a corpus whose gitignored directory holds a markdown file
        // linking back into `docs/specs/` — it produces no diagnostic of
        // any kind, because the walk never visits it.
        let dir = tempdir();
        dir.git_init();
        dir.write(".gitignore", ".scratch/\n");
        dir.write(
            "docket.ncl",
            r#"{ genres = [ { path = "docs/specs/**", kinds = ["constraint"], quadrant = "reference" } ] }"#,
        );
        dir.write(
            "docs/specs/x.md",
            "### [x]\n\n```claim\nkind: constraint\nevaluator: test\n```\n",
        );
        dir.write(".scratch/notes.md", "See [x](../docs/specs/x.md#x).\n");
        let report = run(&dir);
        assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    }

    #[test]
    fn a_registered_bold_form_claim_passes_every_check_cleanly() {
        let dir = tempdir();
        dir.write(
            "docket.ncl",
            r#"{
              genres = [
                { path = "docs/specs/**", kinds = ["constraint"], quadrant = "reference" },
                { path = "docs/models/**", kinds = ["invariant"], quadrant = "reference" },
              ],
            }"#,
        );
        dir.write("docs/models/composition-model.md", "## 6. The fact-set\n");
        dir.write(
            "docs/specs/x.md",
            "**[bold-registered]**: A bold-form claim that depends on\na model section, declared via a real relative markdown link.\n\nSee [the fact-set](../models/composition-model.md#6).\n\n```claim\nkind: constraint\nevaluator: test\ndepends: [docs/models/composition-model#6]\n```\n",
        );
        let report = run(&dir);
        assert!(report.passed(), "{:#?}", report.diagnostics);
        assert!(
            only(&report, "unregistered-definition").is_empty(),
            "{:#?}",
            report.diagnostics
        );
    }

    // --- dangling-reference ------------------------------------------------
    //
    // `.ledger/2026-08-05-links-are-document-facts-not-claim-attributes.md`:
    // links are collected and resolved per DOCUMENT now, not only within a
    // claim's C5 scope. These tests cover the new surface; C5's own
    // claim-scoped tests above are untouched and still pin its behavior.

    #[test]
    fn a_document_with_no_claims_at_all_still_has_its_links_resolved() {
        // The dispatch's core deliverable: a claimless guide has no C5
        // scope to collect a link into, so before this change its links
        // went nowhere — not even a check ever saw them. This document
        // declares no claim block anywhere.
        let dir = tempdir();
        dir.write(
            "docket.ncl",
            r#"{ genres = [ { path = "docs/guides/**", kinds = [], quadrant = "how-to" } ] }"#,
        );
        dir.write(
            "docs/guides/setup.md",
            "# Setup\n\nSee [the spec](does-not-exist) for background.\n",
        );
        let report = run(&dir);
        let findings = only(&report, "dangling-reference");
        assert_eq!(findings.len(), 1, "{:#?}", report.diagnostics);
        assert_eq!(findings[0].file, "docs/guides/setup.md");
        assert_eq!(findings[0].severity, Severity::Warn);
        assert!(
            report.passed(),
            "dangling-reference must never fail the report: {:#?}",
            report.diagnostics
        );
    }

    #[test]
    fn a_document_with_no_claims_and_a_resolving_link_produces_no_finding() {
        let dir = tempdir();
        dir.write(
            "docket.ncl",
            r#"{
              genres = [
                { path = "docs/guides/**", kinds = [], quadrant = "how-to" },
                { path = "docs/specs/**", kinds = ["constraint"], quadrant = "reference" },
              ],
            }"#,
        );
        dir.write(
            "docs/specs/a.md",
            "### [target]\n\n```claim\nkind: constraint\nevaluator: test\n```\n",
        );
        dir.write(
            "docs/guides/setup.md",
            "# Setup\n\nSee [the spec](../specs/a#target) for background.\n",
        );
        let report = run(&dir);
        assert!(
            only(&report, "dangling-reference").is_empty(),
            "{:#?}",
            report.diagnostics
        );
    }

    #[test]
    fn a_link_outside_a_claims_scope_never_satisfies_its_declaration_even_though_it_resolves() {
        // The load-bearing constraint the dispatch names explicitly: C5's
        // claim-scoping must survive unchanged even though resolution now
        // runs over the whole document. The link sits in [a]'s prose, well
        // before [b]'s heading — document-wide, it resolves fine (the
        // claim it names is real), but [b]'s own `depends` on the same id
        // must still fail C5, because nothing in [b]'s own scope links it.
        let dir = tempdir();
        dir.write(
            "docket.ncl",
            r#"{ genres = [ { path = "docs/specs/**", kinds = ["constraint"], quadrant = "reference" } ] }"#,
        );
        dir.write(
            "docs/specs/x.md",
            "### [target-claim]\n\n```claim\nkind: constraint\nevaluator: test\n```\n\n\
             ### [a]\n\nSee [it](target-claim).\n\n\
             ### [b]\n\n```claim\nkind: constraint\nevaluator: test\ndepends: [target-claim]\n```\n",
        );
        let report = run(&dir);
        let failures = only(&report, "C5");
        assert_eq!(failures.len(), 1, "{:#?}", report.diagnostics);
        assert!(failures[0].message.contains("target-claim"));
        // The link itself, read document-wide, resolves — no
        // dangling-reference finding for it.
        assert!(
            only(&report, "dangling-reference").is_empty(),
            "{:#?}",
            report.diagnostics
        );
    }

    #[test]
    fn a_gitignored_link_fires_unreachable_reference_only_never_also_dangling() {
        // `unreachable-reference` already covers the gitignored case
        // (O4's "unreachable", distinct from "dangles") — reused, not
        // duplicated, per the dispatch.
        let dir = tempdir();
        dir.git_init();
        dir.write(".gitignore", ".scratch/\n");
        dir.write(
            "docket.ncl",
            r#"{ genres = [ { path = "docs/specs/**", kinds = ["constraint"], quadrant = "reference" } ] }"#,
        );
        dir.write(
            "docs/specs/x.md",
            "### [x]\n\nSee [notes](../../.scratch/notes.md).\n\n```claim\nkind: constraint\nevaluator: test\n```\n",
        );
        let report = run(&dir);
        assert_eq!(
            only(&report, "unreachable-reference").len(),
            1,
            "{:#?}",
            report.diagnostics
        );
        assert!(
            only(&report, "dangling-reference").is_empty(),
            "{:#?}",
            report.diagnostics
        );
    }

    // --- evaluator: absent / absent-marker-stale --------------------------

    #[test]
    fn evaluator_absent_is_accepted_by_c1() {
        let dir = tempdir();
        dir.write(
            "docket.ncl",
            r#"{ genres = [ { path = "docs/specs/**", kinds = ["constraint"], quadrant = "reference" } ] }"#,
        );
        dir.write(
            "docs/specs/x.md",
            "### [no-retry-header]\n\nThere is no `Retry-After` header.\n\n```claim\nkind: constraint\nevaluator: absent\n```\n",
        );
        let report = run(&dir);
        assert!(only(&report, "C1").is_empty(), "{:#?}", report.diagnostics);
    }

    #[test]
    fn a_fresh_absence_marker_passes_every_check_cleanly() {
        let dir = tempdir();
        dir.write(
            "docket.ncl",
            r#"{ genres = [ { path = "docs/specs/**", kinds = ["constraint"], quadrant = "reference" } ] }"#,
        );
        dir.write(
            "docs/specs/x.md",
            "### [no-retry-header]\n\nThere is no `Retry-After` header.\n\n\
             <!--\n@docket: no-retry-header :: Retry-After\n-->\n\n\
             ```claim\nkind: constraint\nevaluator: absent\n```\n",
        );
        let report = run(&dir);
        assert!(report.passed(), "{:#?}", report.diagnostics);
        assert!(
            only(&report, "absent-marker-stale").is_empty(),
            "{:#?}",
            report.diagnostics
        );
    }

    #[test]
    fn absent_marker_stale_fires_when_the_prose_no_longer_names_the_literal() {
        let dir = tempdir();
        dir.write(
            "docket.ncl",
            r#"{ genres = [ { path = "docs/specs/**", kinds = ["constraint"], quadrant = "reference" } ] }"#,
        );
        dir.write(
            "docs/specs/x.md",
            "### [no-retry-header]\n\nThe old note about this is gone now.\n\n\
             <!--\n@docket: no-retry-header :: Retry-After\n-->\n\n\
             ```claim\nkind: constraint\nevaluator: absent\n```\n",
        );
        let report = run(&dir);
        let warnings = only(&report, "absent-marker-stale");
        assert_eq!(warnings.len(), 1, "{:#?}", report.diagnostics);
        assert_eq!(warnings[0].severity, Severity::Warn);
        assert!(warnings[0].message.contains("Retry-After"));
        assert!(
            report.passed(),
            "absent-marker-stale must never flip the exit code: {:#?}",
            report.diagnostics
        );
    }

    #[test]
    fn absent_marker_stale_is_silent_for_a_marker_in_a_different_file_from_its_claim() {
        let dir = tempdir();
        dir.write(
            "docket.ncl",
            r#"{ genres = [ { path = "docs/specs/**", kinds = ["constraint"], quadrant = "reference" } ] }"#,
        );
        dir.write(
            "docs/specs/x.md",
            "### [no-retry-header]\n\nThere is no `Retry-After` header.\n\n```claim\nkind: constraint\nevaluator: absent\n```\n",
        );
        // The marker lives in src/, a different file than the claim's
        // own document — no prose scope to compare it against, so this
        // must never fire (src/absence.rs, `find_stale_markers`).
        dir.write("src/lib.rs", "// @docket: no-retry-header :: Retry-After\n");
        let report = run(&dir);
        assert!(
            only(&report, "absent-marker-stale").is_empty(),
            "{:#?}",
            report.diagnostics
        );
    }

    // --- the index -------------------------------------------------------

    #[test]
    fn the_index_matches_the_mvp_example_shape() {
        let dir = tempdir();
        dir.write(
            "docket.ncl",
            r#"{
              genres = [
                { path = "docs/specs/**", kinds = ["constraint"], quadrant = "reference" },
                { path = "docs/models/**", kinds = ["invariant"], quadrant = "reference" },
                { path = "docs/architecture/**", kinds = ["invariant"], quadrant = "reference" },
              ],
            }"#,
        );
        dir.write(
            "docs/specs/lock-file-schema.md",
            "### [lock-groundness]\n\nEvery lock value MUST be ground: names bound to content identities and exact version strings.\n\nSee [the fact-set](../models/composition-model.md#6) and [execution](../architecture/execution-model.md#2.4).\n\n```claim\nkind: constraint\nevaluator: property-test\ndepends: [docs/models/composition-model#6, docs/architecture/execution-model#2.4]\n```\n",
        );
        dir.write(
            "docs/models/composition-model.md",
            "## 6. The fact-set: the substrate's only state\n",
        );
        dir.write(
            "docs/architecture/execution-model.md",
            "## 2.4 Identity discipline\n",
        );

        let config = load_config(dir.path()).unwrap();
        let loaded = load_corpus(dir.path(), &config).unwrap();
        let markers = crate::marker::scan_markers(dir.path()).unwrap();
        let result = run_checks(&loaded, &config, &register_path(), &markers).unwrap();
        assert!(result.report.passed(), "{:#?}", result.report.diagnostics);

        let claim = result
            .index
            .claims
            .get("lock-groundness")
            .expect("claim indexed");
        assert_eq!(claim.file, "docs/specs/lock-file-schema.md");
        assert_eq!(claim.kind, "constraint");
        assert_eq!(claim.evaluator, "property-test");
        assert_eq!(
            claim.depends,
            vec![
                "docs/models/composition-model#6",
                "docs/architecture/execution-model#2.4"
            ]
        );
        assert!(claim.because.is_empty());

        let doc = result
            .index
            .documents
            .get("docs/specs/lock-file-schema")
            .expect("document indexed");
        assert_eq!(doc.file, "docs/specs/lock-file-schema.md");
        assert_eq!(doc.genre, "docs/specs/**");
    }
}
