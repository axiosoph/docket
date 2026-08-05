//! `docket run <claim-id>`: given a claim, execute what its evaluator
//! names (marker.rs locates it) and report pass / fail / absent.
//!
//! Deliberately **per-claim** (`docket run <id>`, mirroring `docket
//! blast <ref>`'s shape), not a whole-corpus table — a table over every
//! claim is the verdict register, explicitly out of this dispatch's
//! scope ("Build only the runner").
//!
//! **Three outcomes for a claim that names a real evaluator, two more
//! for a claim that doesn't run anything at all:**
//!
//! - [`Outcome::Absent`] — no marker for this claim id exists anywhere
//!   in the corpus. A broken *grade*: the claim asserts a kind of
//!   evidence that was never actually produced.
//! - [`Outcome::Fail`] — a marker exists and its command exited
//!   non-zero. A real *defect*: the evidence was attempted and did not
//!   hold.
//! - [`Outcome::Pass`] — a marker exists, every one of its commands
//!   exited zero, and none of them is a recognized vacuity (below).
//! - [`Outcome::None`] — `evaluator: none`. Not a grade at all;
//!   README.md is explicit that `none` "denotes unimplemented, which is
//!   an honest and common state, not a missing field," so this case
//!   never touches the marker scan and needs nothing executable to be
//!   reported — the marker index isn't even consulted.
//! - [`Outcome::Review`] — `evaluator: review`. Mechanically treated the
//!   same way as `none` (the marker index isn't consulted — there is
//!   nothing to run), but **not** the same claim about the world:
//!   `none` asserts nothing has been done, `review` asserts a human or
//!   agent read the claim against its target and it holds
//!   (`.ledger/log/2026-07-29-review-is-the-vouch.md`). Kept a distinct
//!   variant, distinct printed outcome, so a reader (or a script parsing
//!   `docket run`'s output) can never mistake a vouch for a gap.
//! - [`Outcome::Vacuous`] — every marker's command exited zero, but at
//!   least one of them **checked nothing**: the exit status says
//!   success, but the runner recognizes the output's own shape as
//!   proof nothing was actually exercised. Measured, not hypothetical:
//!   `cargo test <a-typo-or-deleted-name> -- --exact` and `cargo test
//!   <an-#[ignore]d-name> -- --exact` both exit 0 with `0 passed; 0
//!   failed` in their summary line — a marker naming a typo'd, renamed,
//!   deleted, or `#[ignore]`d test would otherwise report `Pass`. This
//!   is what the dispatch that added this outcome called "green-by-
//!   construction inside the tool built to prevent green-by-
//!   construction," and it is distinct from `Fail` for the same reason
//!   `Absent` is: "write the marker" (the id is real, the command is
//!   wrong), "the evidence held" (`Pass`), "the evidence broke"
//!   (`Fail`), and "nothing was evidence at all" (`Vacuous`) are four
//!   different diagnoses, not three plus a special case.
//!
//!   Detection is necessarily per-tool (see [`detect_vacuity`] and
//!   [`VACUITY_SIGNALS`] for the exact signal and the reasoning behind
//!   a small table rather than a config format) and only ever
//!   *downgrades* a success — a nonzero exit is already `Fail` and
//!   never becomes `Vacuous` regardless of what its output looks like.
//!   An unrecognized tool's successful output is `Pass`, not `Vacuous`
//!   — see the same doc comment for why treating today's unknowns as
//!   guilty would break fixtures this dispatch was required to leave
//!   unchanged (`run-pass`'s `true`, which has no recognizable shape at
//!   all). A marker can opt out of detection entirely with a trailing
//!   `!` on its id (`marker.rs`) — a deliberate, once-written assertion
//!   for an evaluator kind the table has no recognizer for, never a
//!   silent default.
//!
//! **A bare marker (`marker.rs`'s "The bare form") is evidence only for a
//! `type`-graded claim.** `type` is discharged by a marked item's
//! existence, not by running anything — the marker's own presence is the
//! full check this runner can make (README.md, "Where type-discharge's
//! honesty limit sits": the machine confirms the marked item exists,
//! never that it enforces what the claim says — that half stays
//! review-established). So a bare marker on a `type`-graded claim is
//! `Pass` on sight, no command spawned. Every other mechanical grade
//! promises evidence a command produces, and a bare marker has no command
//! to offer it — so for those grades a bare marker is filtered out before
//! it is even considered a match, and a claim backed only by bare
//! markers reports `Absent` exactly as if nothing had been written. No
//! sixth outcome for this: the fix is "add a command," which is what
//! `Absent` already tells a reader to do (see [`run_claim`]'s doc
//! comment, and the sigil-migration precedent it follows).
//!
//! Absent and fail are kept structurally distinct rather than folded
//! into one "not discharged" result, per the dispatch: "a wrong grade
//! costs a code re-read to fix" and the two diagnoses send a reader in
//! different directions (write the marker vs. fix the evaluator) —
//! collapsing them is named as "the failure most likely to be
//! introduced here." The same argument is why `Vacuous` is its own
//! state rather than a flavor of `Fail`: "the command exited non-zero"
//! and "the command exited zero but checked nothing" send a reader in
//! different directions too (fix the evaluator vs. fix the marker's
//! target), and folding the newer, rarer case into the one that already
//! has a well-understood meaning would make `Fail` itself lie half the
//! time.
//!
//! **Exit codes** (see `main.rs`): `0` pass/none/review, `1` fail, `2` usage
//! error (unknown claim id, corpus/config failed to load — the existing
//! meaning MVP.md §5 and `blast`'s own "ref does not resolve" already
//! give exit 2), `3` absent, `4` vacuous. `check` deliberately does not
//! fail a run on a non-empty *report* — its exit code tracks
//! `Severity::Fail` specifically, so a report holding only
//! lower-severity diagnostics still exits 0 (checks.rs,
//! `CheckReport::passed`). That reasoning does not carry over unchanged
//! here: `check` aggregates many independent diagnostics of differing
//! severity into one process exit, so severity has real work to do
//! collapsing them. `run` reports on exactly one claim, and its outcome
//! already *is* the severity — there is nothing left to aggregate, and
//! a distinct exit code per outcome is strictly more information for a
//! caller (a CI gate can tell "write the marker" from "the test
//! regressed" from "the test checked nothing" without parsing stdout)
//! at no cost, since there is only ever one outcome to encode per
//! invocation.

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
    /// `evaluator: review` — a human or agent read the claim and it
    /// holds. Distinct from `None`: this asserts the claim is true,
    /// `None` asserts nothing has been attempted. Distinct from `Pass`:
    /// this is testimony (a vouch), not a re-runnable check
    /// (a corroboration) — see the module docs.
    Review,
    /// Every marker's command exited zero, but at least one of them is
    /// recognized as having verified nothing — see [`detect_vacuity`].
    /// Distinct from `Pass`: a command that ran and checked nothing is
    /// not evidence, even though its exit status says success.
    Vacuous,
}

impl Outcome {
    pub fn as_str(&self) -> &'static str {
        match self {
            Outcome::Pass => "pass",
            Outcome::Fail => "fail",
            Outcome::Absent => "absent",
            Outcome::None => "none",
            Outcome::Review => "review",
            Outcome::Vacuous => "vacuous",
        }
    }
}

/// A named recognizer for a per-binary test summary block, paired with
/// the substring that marks the start of one such block in a tool's
/// output and the substring within a block that means "this block
/// checked nothing."
///
/// **Why block-scoped, not whole-output.** `cargo test` on a crate with
/// both `#[cfg(test)] mod tests` and doc-comment examples runs *two*
/// binaries in one invocation — the unittest binary, then the doctest
/// binary — each printing its own `test result: ` summary line.
/// Matching the vacuity substring against the whole of `stdout` (this
/// table's original shape) means the doctest binary's legitimate `0
/// passed; 0 failed` — nothing there to run, or nothing there matching
/// a name filter — sinks the *entire* invocation to `Vacuous` even when
/// the unittest binary's block, elsewhere in the same output, reports
/// real passes. That is most idiomatic Rust libraries, not an edge
/// case: measured against a real corpus, every marker on a crate with
/// both kinds of test came back a false-positive `Vacuous`. Segmenting
/// `stdout` into blocks by `block_marker` and requiring *every*
/// recognized block to carry `vacuous_marker` (see [`detect_vacuity`])
/// is what lets "0 tests in the doctest phase, 17 in the unittest
/// phase" read as `Pass`, the way it should — a `0 tests` block sitting
/// alongside a block that ran fifteen is normal and expected, not
/// evidence of anything.
///
/// **Why a table, not a config file.** Every recognizer here is a
/// substring test — cheap, and precise enough that a fourth tool costs
/// one entry, not a rewrite. A declarative format loaded from
/// `docket.ncl` or a sibling file was considered and rejected: only one
/// signal is actually measured against a real corpus today (cargo's),
/// the marker grammar this feeds (`marker.rs`) already lives in the
/// contract-free half of the corpus (a marker is source-code text, not
/// YAML the Nickel contract validates), and `docket.ncl`/`contracts/**`
/// are out of this dispatch's edit surface besides. A Rust table is the
/// minimal representation that is still declarative in the sense that
/// matters: adding a signal is adding a row, never touching the
/// matching logic.
///
/// **Why one entry covers both the missing-test and the `#[ignore]`d-test
/// case.** Measured directly (see the dispatch): `cargo test
/// nonexistent -- --exact` prints `running 0 tests` and a summary line
/// `test result: ok. 0 passed; 0 failed; 0 ignored; ...`. Naming an
/// `#[ignore]`d test by its exact name is a *different* shape —
/// cargo does collect and print `running 1 test`, then reports it
/// `ignored` — so `running 0 tests` alone misses it. What both share,
/// and what a genuine pass never has, is the summary line's counts:
/// `0 passed; 0 failed`. Zero of each is true exactly when nothing that
/// ran was actually exercised to a verdict — a filtered-to-nothing run
/// and a filtered-to-only-ignored run both land there, and any run with
/// at least one real pass or failure never does.
struct VacuitySignal {
    name: &'static str,
    block_marker: &'static str,
    vacuous_marker: &'static str,
}

const VACUITY_SIGNALS: &[VacuitySignal] = &[VacuitySignal {
    name: "cargo test collected nothing to run (or only #[ignore]d tests)",
    block_marker: "test result: ",
    vacuous_marker: "0 passed; 0 failed",
}];

/// Whether every recognized summary block in `stdout` shows nothing was
/// exercised.
///
/// - **Zero recognized blocks** (no line contains a known
///   `block_marker`) is an unrecognized shape, not a vacuity — treated
///   as `Pass`, same as before this fix (see the module docs'
///   "Whether an unrecognised tool's output is a pass or a vacuity"
///   decision): fixtures already prove `sh` builtins like `true` must
///   still report `Pass`, and those have no recognizable shape at all.
/// - **One or more recognized blocks, all vacuous-shaped**, is
///   `Vacuous` — the case this detector exists for.
/// - **One or more recognized blocks, at least one carrying real
///   activity** (its line doesn't match `vacuous_marker`), is `Pass` —
///   a run cannot be downgraded by a block that checked nothing sitting
///   next to a block that didn't.
fn detect_vacuity(stdout: &str) -> Option<&'static str> {
    VACUITY_SIGNALS.iter().find_map(|signal| {
        let mut blocks = stdout
            .lines()
            .filter(|line| line.contains(signal.block_marker))
            .peekable();
        blocks.peek()?;
        blocks
            .all(|line| line.contains(signal.vacuous_marker))
            .then_some(signal.name)
    })
}

/// One marker, resolved: a commanded marker's command executed and
/// captured, or a bare marker taken as located (see [`run_claim`]'s "bare
/// marker" handling — reachable only when this outcome's `marker.command`
/// is `None`, which by construction means the claim's evaluator is
/// `type`).
#[derive(Debug, Clone)]
pub struct MarkerOutcome {
    pub marker: Marker,
    pub success: bool,
    /// `None` when the process was killed by a signal rather than
    /// exiting (`std::process::ExitStatus::code()`'s own contract), or
    /// when `marker.command` is `None` — a bare marker has no process to
    /// report a code for. `success` disambiguates the two: a bare marker
    /// is always `success: true`.
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    /// `Some(signal name)` when `success` is true, the marker is not
    /// [`Marker::exempt`], and `stdout` matched a recognized entry in
    /// [`VACUITY_SIGNALS`]. Always `None` for a failed command — a
    /// command that already reports failure needs no second diagnosis —
    /// and always `None` for an exempt marker, by construction.
    pub vacuous: Option<&'static str>,
}

#[derive(Debug, Clone)]
pub struct RunResult {
    pub claim_id: ClaimId,
    pub evaluator: String,
    pub outcome: Outcome,
    /// Empty for [`Outcome::Absent`] (nothing ran) and
    /// [`Outcome::None`]/[`Outcome::Review`] (nothing was even looked
    /// for). One entry per matching marker for
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

    // `review`: testimony, not a re-runnable check (module docs, and
    // `.ledger/log/2026-07-29-review-is-the-vouch.md`) — there is nothing
    // for this runner to execute, by definition, so the marker index is
    // never consulted, mirroring `none`'s short-circuit above. The
    // outcome is deliberately its own variant rather than reusing `None`:
    // a `review` claim is asserted TRUE (a vouch), while `none` asserts
    // nothing has been attempted — collapsing them would erase exactly
    // the distinction this evaluator exists to add.
    if evaluator == "review" {
        return Ok(RunResult {
            claim_id: claim.id.clone(),
            evaluator,
            outcome: Outcome::Review,
            markers: Vec::new(),
        });
    }

    // A bare marker (no `:: <command>`, marker.rs) is evidence only for a
    // `type`-graded claim — `type` is discharged by the marked item's
    // existence, and locating that marker *is* the check (module docs).
    // For every other grade the claim promises evidence a command
    // produces, which a bare marker cannot offer, so it is filtered out
    // here rather than reaching the loop below: a claim backed only by
    // bare markers then falls straight through to `Absent`, the same
    // outcome a claim with no marker at all gets — deliberately no
    // separate diagnostic (module docs, and the sigil-migration
    // precedent it follows: `Absent` already means "write the marker,"
    // and adding a command to an existing bare one is that fix).
    let type_grade = evaluator == "type";
    let matching: Vec<&Marker> = markers
        .iter()
        .filter(|m| m.id == claim.id)
        .filter(|m| type_grade || m.command.is_some())
        .collect();
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
        let Some(command) = &m.command else {
            // Bare marker on a `type`-graded claim (the only way it
            // reached this loop, by the filter above): nothing to run,
            // nothing to diagnose. The marker's presence in the scan is
            // the entire check — nothing here can also confirm the type
            // enforces what the claim says, which stays review-established
            // (README.md, "Where type-discharge's honesty limit sits").
            outcomes.push(MarkerOutcome {
                marker: m.clone(),
                success: true,
                exit_code: None,
                stdout: String::new(),
                stderr: String::new(),
                vacuous: None,
            });
            continue;
        };

        // A shell, not a direct exec: a marker's command is free-form
        // (pipes, `--` flags, shell-quoted arguments an author wrote by
        // hand) exactly because it is meant to be whatever its author
        // would type at a prompt to run their own evaluator, not a
        // pre-tokenized argv this crate would have to parse.
        let output = Command::new("sh")
            .arg("-c")
            .arg(command)
            .current_dir(corpus_root)
            .output()
            .map_err(RunError::Spawn)?;
        let success = output.status.success();
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        // Only a *successful* command needs a second look: a nonzero
        // exit is already `Fail`, and a real failure's own exit status
        // is diagnosis enough. An exempt marker (`@docket: <id>! :: …`)
        // skips this unconditionally — the author's deliberate assertion
        // that the exit status alone is conclusive for this evaluator.
        let vacuous = if success && !m.exempt {
            detect_vacuity(&stdout)
        } else {
            None
        };
        outcomes.push(MarkerOutcome {
            marker: m.clone(),
            success,
            exit_code: output.status.code(),
            stdout,
            stderr,
            vacuous,
        });
    }

    // Multiple markers can name the same claim (a property test plus a
    // doctest, say) — the claim is discharged only if every one of them
    // is, the same all-of-them reading `depends`/`because`'s own
    // resolution gives a set of targets. `Fail` outranks `Vacuous`: a
    // real failure is a stronger diagnosis than "nothing was checked",
    // and both outrank `Pass`, which requires every marker to have
    // actually verified something.
    let outcome = if !outcomes.iter().all(|o| o.success) {
        Outcome::Fail
    } else if outcomes.iter().any(|o| o.vacuous.is_some()) {
        Outcome::Vacuous
    } else {
        Outcome::Pass
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
            command: Some(command.to_string()),
            file: "src/lib.rs".to_string(),
            line: Line(1),
            exempt: false,
        }
    }

    fn exempt_marker(id: &str, command: &str) -> Marker {
        Marker {
            exempt: true,
            ..marker(id, command)
        }
    }

    fn bare_marker(id: &str) -> Marker {
        Marker {
            id: id.to_string(),
            command: None,
            file: "src/lib.rs".to_string(),
            line: Line(1),
            exempt: false,
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
    fn evaluator_review_never_consults_markers() {
        let corpus =
            corpus_with("### [x]\n\n```claim\nkind: requirement\nevaluator: review\n```\n");
        // A marker for `x` exists but must never be looked at — same
        // shape as `evaluator_none_never_consults_markers`, but `review`
        // must report its own outcome, not `None`'s.
        let markers = vec![marker("x", "false")];
        let result = run_claim(&corpus, "x", Path::new("."), &markers).unwrap();
        assert_eq!(result.outcome, Outcome::Review);
        assert!(result.markers.is_empty());
    }

    #[test]
    fn evaluator_review_is_distinct_from_none() {
        // The whole point of the grade: `none` and `review` must never
        // collapse to the same outcome, even with no marker in sight for
        // either.
        let corpus =
            corpus_with("### [x]\n\n```claim\nkind: requirement\nevaluator: review\n```\n");
        let result = run_claim(&corpus, "x", Path::new("."), &[]).unwrap();
        assert_eq!(result.outcome, Outcome::Review);
        assert_ne!(result.outcome, Outcome::None);
        assert_eq!(result.evaluator, "review");
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

    /// The literal line real `cargo test <nonexistent> -- --exact` prints
    /// (measured directly, not assumed): zero collected, so nothing ran.
    const CARGO_ZERO_COLLECTED: &str = "test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 3 filtered out; finished in 0.00s\n";
    /// The literal line real `cargo test <ignored-test> -- --exact` prints
    /// — a different shape (the test *is* collected, then skipped), that
    /// shares the same zero-passed/zero-failed counts.
    const CARGO_ONLY_IGNORED: &str = "test result: ok. 0 passed; 0 failed; 1 ignored; 0 measured; 2 filtered out; finished in 0.00s\n";

    #[test]
    fn a_marker_naming_a_nonexistent_test_reports_vacuous_not_pass() {
        let corpus = corpus_with("### [x]\n\n```claim\nkind: constraint\nevaluator: test\n```\n");
        let markers = vec![marker(
            "x",
            &format!("printf '%s' '{CARGO_ZERO_COLLECTED}'"),
        )];
        let result = run_claim(&corpus, "x", Path::new("."), &markers).unwrap();
        assert_eq!(result.outcome, Outcome::Vacuous);
        // The command itself still exited 0 — the exit status was never
        // the lie's detection point, its content was.
        assert!(result.markers[0].success);
        assert_eq!(
            result.markers[0].vacuous,
            Some("cargo test collected nothing to run (or only #[ignore]d tests)")
        );
    }

    #[test]
    fn a_marker_naming_an_ignored_test_reports_vacuous_not_pass() {
        let corpus = corpus_with("### [x]\n\n```claim\nkind: constraint\nevaluator: test\n```\n");
        let markers = vec![marker("x", &format!("printf '%s' '{CARGO_ONLY_IGNORED}'"))];
        let result = run_claim(&corpus, "x", Path::new("."), &markers).unwrap();
        assert_eq!(result.outcome, Outcome::Vacuous);
        assert!(result.markers[0].success);
        assert!(result.markers[0].vacuous.is_some());
    }

    #[test]
    fn a_failing_marker_is_fail_even_if_its_own_output_would_also_read_as_vacuous() {
        // Fail must outrank Vacuous: a command that both exits nonzero
        // and happens to print a vacuous-shaped line is a real defect,
        // not "nothing was checked" — and vacuity is only ever computed
        // on a successful exit in the first place (see the `success &&
        // !m.exempt` guard), so this also pins that the detector never
        // even runs on this path.
        let corpus = corpus_with("### [x]\n\n```claim\nkind: constraint\nevaluator: test\n```\n");
        let markers = vec![marker(
            "x",
            &format!("printf '%s' '{CARGO_ZERO_COLLECTED}'; exit 1"),
        )];
        let result = run_claim(&corpus, "x", Path::new("."), &markers).unwrap();
        assert_eq!(result.outcome, Outcome::Fail);
        assert_eq!(result.markers[0].vacuous, None);
    }

    #[test]
    fn an_exempt_marker_bypasses_vacuity_detection_and_still_reports_pass() {
        let corpus = corpus_with("### [x]\n\n```claim\nkind: constraint\nevaluator: test\n```\n");
        let markers = vec![exempt_marker(
            "x",
            &format!("printf '%s' '{CARGO_ZERO_COLLECTED}'"),
        )];
        let result = run_claim(&corpus, "x", Path::new("."), &markers).unwrap();
        assert_eq!(result.outcome, Outcome::Pass);
        assert_eq!(result.markers[0].vacuous, None);
    }

    #[test]
    fn a_unittest_block_with_real_passes_outranks_a_vacuous_doctest_block() {
        // The real-world shape this fix exists for: a single `cargo
        // test` invocation prints one `test result: ` block per binary.
        // A crate with both unit tests and doc-comment examples runs
        // two — the unittest binary here reports 17 real passes, the
        // doctest binary (measured separately, see run-vacuous-missing)
        // reports the same `0 passed; 0 failed` shape a renamed/deleted
        // test does. The claim must still discharge: one block reporting
        // real activity means the invocation is not vacuous, full stop.
        let corpus = corpus_with("### [x]\n\n```claim\nkind: constraint\nevaluator: test\n```\n");
        let output = "running 17 tests\n\
             test result: ok. 17 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n\
             \n\
             running 0 tests\n\
             \n\
             test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 1 filtered out; finished in 0.00s\n";
        let markers = vec![marker("x", &format!("printf '%s' '{output}'"))];
        let result = run_claim(&corpus, "x", Path::new("."), &markers).unwrap();
        assert_eq!(result.outcome, Outcome::Pass);
        assert_eq!(result.markers[0].vacuous, None);
    }

    #[test]
    fn multiple_all_vacuous_blocks_still_report_vacuous() {
        // The aggregation itself, pinned separately from the
        // single-block case (run-vacuous-missing/-ignored): two
        // recognized blocks, neither carrying real activity, must still
        // downgrade — "many blocks" is not itself grounds for `Pass`,
        // only a block with real activity is.
        let corpus = corpus_with("### [x]\n\n```claim\nkind: constraint\nevaluator: test\n```\n");
        let output = "test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 1 filtered out; finished in 0.00s\n\
             test result: ok. 0 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.00s\n";
        let markers = vec![marker("x", &format!("printf '%s' '{output}'"))];
        let result = run_claim(&corpus, "x", Path::new("."), &markers).unwrap();
        assert_eq!(result.outcome, Outcome::Vacuous);
    }

    #[test]
    fn a_marker_with_no_recognized_output_shape_still_passes() {
        // The `true`/`false` fixtures already pin this indirectly; this
        // test pins the decision itself (see the module docs' fourth
        // decision): an unrecognized tool's output is a pass, not a
        // vacuity, so a shell builtin or any tool this runner has no
        // recognizer for keeps working exactly as it did before this
        // change.
        let corpus = corpus_with("### [x]\n\n```claim\nkind: constraint\nevaluator: test\n```\n");
        let markers = vec![marker("x", "echo 'some other tool, all clear'")];
        let result = run_claim(&corpus, "x", Path::new("."), &markers).unwrap();
        assert_eq!(result.outcome, Outcome::Pass);
    }

    // --- bare markers (marker.rs's "The bare form") -----------------------

    #[test]
    fn a_bare_marker_passes_a_type_graded_claim_without_spawning_anything() {
        let corpus = corpus_with("### [x]\n\n```claim\nkind: constraint\nevaluator: type\n```\n");
        let markers = vec![bare_marker("x")];
        let result = run_claim(&corpus, "x", Path::new("."), &markers).unwrap();
        assert_eq!(result.outcome, Outcome::Pass);
        assert_eq!(result.markers.len(), 1);
        assert!(result.markers[0].success);
        assert_eq!(result.markers[0].exit_code, None);
        assert_eq!(result.markers[0].vacuous, None);
    }

    #[test]
    fn a_bare_marker_on_a_non_type_grade_is_ignored_and_reports_absent() {
        // The claim promises evidence a command produces (`test`); a bare
        // marker cannot offer that, so it does not count as a match at
        // all — the claim reports exactly what it would with no marker
        // written for it.
        let corpus = corpus_with("### [x]\n\n```claim\nkind: constraint\nevaluator: test\n```\n");
        let markers = vec![bare_marker("x")];
        let result = run_claim(&corpus, "x", Path::new("."), &markers).unwrap();
        assert_eq!(result.outcome, Outcome::Absent);
        assert!(result.markers.is_empty());
    }

    #[test]
    fn a_type_graded_claim_can_mix_a_bare_marker_with_a_commanded_one() {
        // An author can still attach a runnable check (e.g. a build
        // sanity command) alongside the bare existence marker — both
        // must hold for the claim to pass, same all-of-them reading as
        // any other multi-marker claim.
        let corpus = corpus_with("### [x]\n\n```claim\nkind: constraint\nevaluator: type\n```\n");
        let markers = vec![bare_marker("x"), marker("x", "true")];
        let result = run_claim(&corpus, "x", Path::new("."), &markers).unwrap();
        assert_eq!(result.outcome, Outcome::Pass);
        assert_eq!(result.markers.len(), 2);
    }

    #[test]
    fn a_type_graded_claim_still_fails_if_its_commanded_marker_fails() {
        // The bare marker's automatic success must not rescue a
        // companion commanded marker that genuinely fails.
        let corpus = corpus_with("### [x]\n\n```claim\nkind: constraint\nevaluator: type\n```\n");
        let markers = vec![bare_marker("x"), marker("x", "false")];
        let result = run_claim(&corpus, "x", Path::new("."), &markers).unwrap();
        assert_eq!(result.outcome, Outcome::Fail);
    }

    #[test]
    fn a_bare_marker_for_a_different_claim_id_does_not_match() {
        let corpus = corpus_with("### [x]\n\n```claim\nkind: constraint\nevaluator: type\n```\n");
        let markers = vec![bare_marker("y")];
        let result = run_claim(&corpus, "x", Path::new("."), &markers).unwrap();
        assert_eq!(result.outcome, Outcome::Absent);
    }
}
