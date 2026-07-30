//! `docket run <claim-id>`: given a claim, execute what its evaluator
//! names (marker.rs locates it) and report pass / fail / absent.
//!
//! Deliberately **per-claim** (`docket run <id>`, mirroring `docket
//! blast <ref>`'s shape), not a whole-corpus table — a table over every
//! claim is the verdict register, explicitly out of this dispatch's
//! scope ("Build only the runner").
//!
//! **Three outcomes for a claim that names a real evaluator, one more
//! for a claim that doesn't:**
//!
//! - [`Outcome::Absent`] — no marker for this claim id exists anywhere
//!   in the corpus. A broken *grade*: the claim asserts a kind of
//!   evidence that was never actually produced.
//! - [`Outcome::Fail`] — a marker exists and its command exited
//!   non-zero. A real *defect*: the evidence was attempted and did not
//!   hold.
//! - [`Outcome::Pass`] — a marker exists and every one of its commands
//!   exited zero.
//! - [`Outcome::None`] — `evaluator: none`. Not a grade at all;
//!   README.md is explicit that `none` "denotes unimplemented, which is
//!   an honest and common state, not a missing field," so this case
//!   never touches the marker scan and needs nothing executable to be
//!   reported — the marker index isn't even consulted.
//!
//! Absent and fail are kept structurally distinct rather than folded
//! into one "not discharged" result, per the dispatch: "a wrong grade
//! costs a code re-read to fix" and the two diagnoses send a reader in
//! different directions (write the marker vs. fix the evaluator) —
//! collapsing them is named as "the failure most likely to be
//! introduced here."
//!
//! **Exit codes** (see `main.rs`): `0` pass/none, `1` fail, `2` usage
//! error (unknown claim id, corpus/config failed to load — the existing
//! meaning MVP.md §5 and `blast`'s own "ref does not resolve" already
//! give exit 2), `3` absent. `check` deliberately does not fail a run
//! on a non-empty *report* — its exit code tracks `Severity::Fail`
//! specifically, so a report holding only lower-severity diagnostics
//! still exits 0 (checks.rs, `CheckReport::passed`). That reasoning
//! does not carry over unchanged here: `check` aggregates many
//! independent diagnostics of differing severity into one process exit,
//! so severity has real work to do collapsing them. `run` reports on
//! exactly one claim, and its outcome already *is* the severity — there
//! is nothing left to aggregate, and a distinct exit code per outcome
//! is strictly more information for a caller (a CI gate can tell "write
//! the marker" from "the test regressed" without parsing stdout) at no
//! cost, since there is only ever one outcome to encode per invocation.

use crate::marker::Marker;
use crate::model::{Claim, ClaimId, Corpus};
use std::path::Path;
use std::process::Command;

#[derive(Debug, thiserror::Error)]
pub enum RunError {
    #[error("no claim with id {0:?} in this corpus")]
    UnknownClaim(ClaimId),
    #[error("could not run `sh` to execute a marker's command: {0}")]
    Spawn(#[source] std::io::Error),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Pass,
    Fail,
    Absent,
    None,
}

impl Outcome {
    pub fn as_str(&self) -> &'static str {
        match self {
            Outcome::Pass => "pass",
            Outcome::Fail => "fail",
            Outcome::Absent => "absent",
            Outcome::None => "none",
        }
    }
}

/// One marker's command, executed and captured.
#[derive(Debug, Clone)]
pub struct MarkerOutcome {
    pub marker: Marker,
    pub success: bool,
    /// `None` when the process was killed by a signal rather than
    /// exiting — `std::process::ExitStatus::code()`'s own contract.
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug, Clone)]
pub struct RunResult {
    pub claim_id: ClaimId,
    pub evaluator: String,
    pub outcome: Outcome,
    /// Empty for [`Outcome::Absent`] (nothing ran) and [`Outcome::None`]
    /// (nothing was even looked for). One entry per matching marker for
    /// [`Outcome::Pass`]/[`Outcome::Fail`].
    pub markers: Vec<MarkerOutcome>,
}

/// Execute `claim_id`'s evaluator and report the outcome.
///
/// `markers` is pre-scanned (by [`crate::marker::scan_markers`]) rather
/// than scanned here, so a caller running several claims against one
/// corpus pays the tree walk once. `corpus_root` is each marker
/// command's working directory, so a command written the way its author
/// would naturally write it (`cargo test …`, `tlc formal/Model.tla`)
/// resolves relative paths against the corpus, not the caller's cwd.
pub fn run_claim(
    corpus: &Corpus,
    claim_id: &str,
    corpus_root: &Path,
    markers: &[Marker],
) -> Result<RunResult, RunError> {
    let claim: &Claim = corpus
        .claims
        .iter()
        .find(|c| c.id == claim_id)
        .ok_or_else(|| RunError::UnknownClaim(claim_id.to_string()))?;

    let evaluator = claim.raw.evaluator.clone().unwrap_or_default();

    // `none`: an honest, unimplemented state (README.md, "What the block
    // holds"). No marker lookup is attempted — nothing executable is
    // required to express it, which the dispatch states as a hard
    // constraint on this case specifically.
    if evaluator == "none" {
        return Ok(RunResult {
            claim_id: claim.id.clone(),
            evaluator,
            outcome: Outcome::None,
            markers: Vec::new(),
        });
    }

    let matching: Vec<&Marker> = markers.iter().filter(|m| m.id == claim.id).collect();
    if matching.is_empty() {
        return Ok(RunResult {
            claim_id: claim.id.clone(),
            evaluator,
            outcome: Outcome::Absent,
            markers: Vec::new(),
        });
    }

    let mut outcomes = Vec::with_capacity(matching.len());
    for m in matching {
        // A shell, not a direct exec: a marker's command is free-form
        // (pipes, `--` flags, shell-quoted arguments an author wrote by
        // hand) exactly because it is meant to be whatever its author
        // would type at a prompt to run their own evaluator, not a
        // pre-tokenized argv this crate would have to parse.
        let output = Command::new("sh")
            .arg("-c")
            .arg(&m.command)
            .current_dir(corpus_root)
            .output()
            .map_err(RunError::Spawn)?;
        outcomes.push(MarkerOutcome {
            marker: m.clone(),
            success: output.status.success(),
            exit_code: output.status.code(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }

    // Multiple markers can name the same claim (a property test plus a
    // doctest, say) — the claim is discharged only if every one of them
    // is, the same all-of-them reading `depends`/`because`'s own
    // resolution gives a set of targets.
    let outcome = if outcomes.iter().all(|o| o.success) {
        Outcome::Pass
    } else {
        Outcome::Fail
    };

    Ok(RunResult {
        claim_id: claim.id.clone(),
        evaluator,
        outcome,
        markers: outcomes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extract::extract_document;
    use crate::model::Line;

    fn corpus_with(src: &str) -> Corpus {
        let result = extract_document("docs/specs/x.md", src);
        Corpus {
            claims: result.claims,
            documents: Vec::new(),
        }
    }

    fn marker(id: &str, command: &str) -> Marker {
        Marker {
            id: id.to_string(),
            command: command.to_string(),
            file: "src/lib.rs".to_string(),
            line: Line(1),
        }
    }

    #[test]
    fn a_marker_whose_command_exits_zero_passes() {
        let corpus = corpus_with("### [x]\n\n```claim\nkind: constraint\nevaluator: test\n```\n");
        let markers = vec![marker("x", "true")];
        let result = run_claim(&corpus, "x", Path::new("."), &markers).unwrap();
        assert_eq!(result.outcome, Outcome::Pass);
        assert_eq!(result.evaluator, "test");
        assert_eq!(result.markers.len(), 1);
        assert!(result.markers[0].success);
    }

    #[test]
    fn a_marker_whose_command_exits_nonzero_fails() {
        // Watched red: `false` always exits 1, so this is the actual
        // failure path exercised, not an assumed one.
        let corpus = corpus_with("### [x]\n\n```claim\nkind: constraint\nevaluator: test\n```\n");
        let markers = vec![marker("x", "false")];
        let result = run_claim(&corpus, "x", Path::new("."), &markers).unwrap();
        assert_eq!(result.outcome, Outcome::Fail);
        assert!(!result.markers[0].success);
        assert_eq!(result.markers[0].exit_code, Some(1));
    }

    #[test]
    fn no_matching_marker_is_absent_not_fail() {
        let corpus = corpus_with("### [x]\n\n```claim\nkind: constraint\nevaluator: test\n```\n");
        let result = run_claim(&corpus, "x", Path::new("."), &[]).unwrap();
        assert_eq!(result.outcome, Outcome::Absent);
        assert!(result.markers.is_empty());
    }

    #[test]
    fn evaluator_none_never_consults_markers() {
        let corpus = corpus_with("### [x]\n\n```claim\nkind: requirement\nevaluator: none\n```\n");
        // A marker for `x` exists but must never be looked at: `none`
        // is unconditionally its own outcome.
        let markers = vec![marker("x", "false")];
        let result = run_claim(&corpus, "x", Path::new("."), &markers).unwrap();
        assert_eq!(result.outcome, Outcome::None);
        assert!(result.markers.is_empty());
    }

    #[test]
    fn an_unknown_claim_id_is_a_run_error() {
        let corpus = corpus_with("### [x]\n\n```claim\nkind: constraint\nevaluator: test\n```\n");
        let err = run_claim(&corpus, "nonexistent", Path::new("."), &[]).unwrap_err();
        assert!(matches!(err, RunError::UnknownClaim(id) if id == "nonexistent"));
    }

    #[test]
    fn all_markers_must_pass_for_the_claim_to_pass() {
        let corpus = corpus_with("### [x]\n\n```claim\nkind: constraint\nevaluator: test\n```\n");
        let markers = vec![marker("x", "true"), marker("x", "false")];
        let result = run_claim(&corpus, "x", Path::new("."), &markers).unwrap();
        assert_eq!(result.outcome, Outcome::Fail);
        assert_eq!(result.markers.len(), 2);
    }

    #[test]
    fn a_marker_for_a_different_claim_id_does_not_match() {
        let corpus = corpus_with("### [x]\n\n```claim\nkind: constraint\nevaluator: test\n```\n");
        let markers = vec![marker("y", "true")];
        let result = run_claim(&corpus, "x", Path::new("."), &markers).unwrap();
        assert_eq!(result.outcome, Outcome::Absent);
    }

    #[test]
    fn marker_commands_run_with_the_corpus_root_as_cwd() {
        let dir = std::env::temp_dir().join(format!(
            "docket-run-cwd-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("marker.txt"), "present\n").unwrap();

        let corpus = corpus_with("### [x]\n\n```claim\nkind: constraint\nevaluator: test\n```\n");
        let markers = vec![marker("x", "test -f marker.txt")];
        let result = run_claim(&corpus, "x", &dir, &markers).unwrap();
        assert_eq!(result.outcome, Outcome::Pass);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
