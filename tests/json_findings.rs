//! CLI-level tests for `docket check`'s `--json` flag (MVP.md §4.1).
//!
//! These invoke the built `docket` binary and assert on its actual
//! stdout/stderr bytes and exit code — the layer a real consumer sees —
//! rather than on `checks::run_checks`'s internal `Vec<Diagnostic>`.
//! `main.rs` owns the rendering; a bug there is invisible to a test one
//! layer below it, which is exactly the class of miss the schema's own
//! worked example (MVP.md §4.1, "Worth stating why it survived a
//! draft") already documents for the index itself.

use std::collections::BTreeSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

fn docket_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_docket"))
}

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures")
}

struct Run {
    stdout: String,
    stderr: String,
    status: std::process::ExitStatus,
}

fn run_check(corpus: &Path, json: bool) -> Run {
    let mut cmd = Command::new(docket_bin());
    cmd.arg("check").arg("--corpus").arg(corpus);
    if json {
        cmd.arg("--json");
    }
    let output = cmd.output().expect("docket binary runs");
    Run {
        stdout: String::from_utf8(output.stdout).expect("stdout is utf8"),
        stderr: String::from_utf8(output.stderr).expect("stderr is utf8"),
        status: output.status,
    }
}

/// A throwaway corpus directory, cleaned up on drop — mirrors
/// `checks.rs`'s own private test helper, reimplemented here since an
/// integration test cannot reach into the crate's `#[cfg(test)]` module.
struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!(
            "docket-json-findings-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn write(&self, relative: &str, contents: &str) {
        let path = self.0.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut f = std::fs::File::create(path).unwrap();
        f.write_all(contents.as_bytes()).unwrap();
    }

    fn git_init(&self) {
        let status = Command::new("git")
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

/// `unreachable-reference` needs a real git repository with a genuinely
/// gitignored, existing target (MVP.md §3, "A candidate must exist on
/// disk") — no static fixture under `fixtures/` sets that precondition
/// up, so it is built here rather than silently skipped.
fn unreachable_reference_corpus() -> TempDir {
    let dir = TempDir::new();
    dir.git_init();
    dir.write(
        "docket.ncl",
        r#"{ genres = [ { path = "docs/specs/**", kinds = ["constraint"], quadrant = "reference" } ] }"#,
    );
    dir.write(".gitignore", ".ledger/\n");
    dir.write(".ledger/note.md", "secret\n");
    dir.write(
        "docs/specs/a.md",
        "### [some-claim]\n\nSee `.ledger/note.md` for detail.\n\n```claim\nkind: constraint\nevaluator: test\n```\n",
    );
    dir
}

#[test]
fn a_clean_corpus_emits_an_empty_findings_array_not_an_absent_key() {
    let run = run_check(&fixtures_dir().join("golden"), true);
    assert!(run.status.success());
    assert!(
        run.stderr.is_empty(),
        "no prose diagnostics under --json, got: {}",
        run.stderr
    );
    let doc: serde_json::Value = serde_json::from_str(&run.stdout).expect("valid JSON");
    let findings = doc.get("findings").expect("findings key present");
    assert_eq!(findings.as_array().expect("findings is an array").len(), 0);
    assert!(doc.get("index").and_then(|i| i.get("claims")).is_some());
    assert!(doc.get("index").and_then(|i| i.get("documents")).is_some());
}

#[test]
fn without_json_the_bare_index_is_emitted_with_no_findings_key() {
    let run = run_check(&fixtures_dir().join("golden"), false);
    assert!(run.status.success());
    let doc: serde_json::Value = serde_json::from_str(&run.stdout).expect("valid JSON");
    assert!(
        doc.get("findings").is_none(),
        "the bare index (no --json) must carry no findings key"
    );
    assert!(doc.get("claims").is_some());
    assert!(doc.get("documents").is_some());
    assert!(run.stderr.is_empty());
}

#[test]
fn json_findings_reproduce_the_prose_diagnostic_exactly() {
    let corpus = fixtures_dir().join("c1-unknown-field");
    let prose = run_check(&corpus, false);
    let json_run = run_check(&corpus, true);

    assert_eq!(prose.status.code(), json_run.status.code());
    assert!(
        json_run.stderr.is_empty(),
        "no prose under --json, got: {}",
        json_run.stderr
    );

    // "error: C1: docs/specs/example.md:3: unknown field(s): extra"
    let line = prose.stderr.lines().next().expect("one diagnostic line");
    let rest = line.strip_prefix("error: ").expect("Fail severity");
    let (check, rest) = rest.split_once(": ").unwrap();
    let (loc, message) = rest.split_once(": ").unwrap();
    let (file, lineno) = loc.rsplit_once(':').unwrap();

    let doc: serde_json::Value = serde_json::from_str(&json_run.stdout).unwrap();
    let findings = doc["findings"].as_array().unwrap();
    assert_eq!(findings.len(), 1);
    let f = &findings[0];
    assert_eq!(f["check"], check);
    assert_eq!(f["severity"], "fail");
    assert_eq!(f["file"], file);
    assert_eq!(f["line"], lineno.parse::<u64>().unwrap());
    assert_eq!(f["message"], message);
    assert_eq!(f["claim_id"], "example-claim");
}

/// Enumerates the real `fixtures/` tree rather than trusting a
/// remembered check list — the exact risk the schema exists to close:
/// a structured output silently narrower than the checks it claims to
/// cover. `unreachable-reference` is added from a purpose-built corpus
/// (above) since no fixture triggers its git precondition statically.
#[test]
fn every_finding_class_the_register_can_emit_appears_through_json_with_correct_claim_id() {
    let mut seen_checks: BTreeSet<String> = BTreeSet::new();
    let mut claim_id_present_for: BTreeSet<String> = BTreeSet::new();
    let mut claim_id_null_for: BTreeSet<String> = BTreeSet::new();

    let mut absorb = |findings: &[serde_json::Value]| {
        for f in findings {
            let obj = f.as_object().expect("finding is an object");
            let mut keys: Vec<&str> = obj.keys().map(String::as_str).collect();
            keys.sort_unstable();
            assert_eq!(
                keys,
                vec!["check", "claim_id", "file", "line", "message", "severity"],
                "finding schema drifted: {f:?}"
            );
            let check = obj["check"].as_str().unwrap().to_string();
            if obj["claim_id"].is_null() {
                claim_id_null_for.insert(check.clone());
            } else {
                claim_id_present_for.insert(check.clone());
            }
            seen_checks.insert(check);
        }
    };

    for entry in std::fs::read_dir(fixtures_dir()).expect("fixtures/ exists") {
        let path = entry.expect("readable dir entry").path();
        if !path.join("docket.ncl").is_file() {
            continue;
        }
        let run = run_check(&path, true);
        // A few fixtures are deliberately a configuration error (exit
        // 2, MVP.md §5) and never reach the diagnostics path at all —
        // e.g. `explanation-forbids-kinds`. Their stdout is prose, not
        // JSON; skip rather than fail on those.
        let Ok(doc) = serde_json::from_str::<serde_json::Value>(&run.stdout) else {
            continue;
        };
        let Some(findings) = doc.get("findings").and_then(|f| f.as_array()) else {
            continue;
        };
        absorb(findings);
    }

    let unreachable_corpus = unreachable_reference_corpus();
    let run = run_check(unreachable_corpus.path(), true);
    let doc: serde_json::Value = serde_json::from_str(&run.stdout).expect("valid JSON");
    absorb(doc["findings"].as_array().expect("findings array"));

    // Confirmed against `contracts/register.ncl`'s `evaluate_impl`
    // diagnostics chain (13 entries) and MVP.md §3 — not recalled.
    let expected: BTreeSet<String> = [
        "C1",
        "C2",
        "C3",
        "C4",
        "C5",
        "orphan-claim",
        "orphaned-because",
        "normative-prose",
        "unregistered-definition",
        "malformed-id",
        "unreachable-reference",
        "absent-marker-stale",
        "dangling-reference",
    ]
    .into_iter()
    .map(String::from)
    .collect();
    assert_eq!(seen_checks, expected, "finding-class coverage drifted");

    // A `claim_id` applies exactly where the diagnostic is about an
    // identified claim (checks.rs's `Diagnostic::claim_id` doc comment):
    // C1-C5 minus orphan-claim (which has no id by construction),
    // `orphaned-because`, and `absent-marker-stale`. Every other check
    // is document- or link-scoped and must never carry one.
    let claim_scoped: BTreeSet<String> = [
        "C1",
        "C2",
        "C3",
        "C4",
        "C5",
        "orphaned-because",
        "absent-marker-stale",
    ]
    .into_iter()
    .map(String::from)
    .collect();
    for check in &claim_scoped {
        assert!(
            claim_id_present_for.contains(check),
            "{check} never carried a non-null claim_id in this fixture sweep"
        );
    }
    for check in expected.difference(&claim_scoped) {
        assert!(
            !claim_id_present_for.contains(check),
            "{check} unexpectedly carried a non-null claim_id"
        );
        assert!(
            claim_id_null_for.contains(check),
            "{check} never appeared in the sweep"
        );
    }
}
