//! A thin CLI over the `docket` library. The checks are pure functions
//! over parsed input (MVP.md §7); this binary is just argument parsing,
//! I/O, and exit codes (§5).

use clap::{Parser, Subcommand};
use docket::model::{CiteRef, anchor_matches};
use docket::run::{Outcome, RunError, RunResult};
use docket::{blast, checks, config, corpus, marker, run};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

#[derive(Parser)]
#[command(
    name = "docket",
    about = "A register of claims across a documentation corpus."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run the checks (C1-C5, orphan-claim, normative-prose,
    /// orphaned-because) and emit the index.
    Check {
        /// Corpus root.
        #[arg(long, default_value = ".")]
        corpus: PathBuf,
        /// Write the index JSON here instead of stdout.
        #[arg(long)]
        out: Option<PathBuf>,
        /// Path to the register evaluator (contracts/register.ncl),
        /// resolved relative to the current directory (it's a
        /// project-level artifact, not per-corpus — see
        /// docket::checks::DEFAULT_REGISTER_RELATIVE_PATH).
        #[arg(long, default_value = checks::DEFAULT_REGISTER_RELATIVE_PATH)]
        register: PathBuf,
    },
    /// Print the transitive blast radius of a ref — a claim id or a
    /// `<doc-path>#<anchor>` document anchor (§4.2, §1.3).
    Blast {
        /// A claim id or a `<doc-path>#<anchor>` document anchor.
        target: String,
        /// Corpus root.
        #[arg(long, default_value = ".")]
        corpus: PathBuf,
    },
    /// Execute a claim's evaluator (a `docket: <id> :: <command>` marker
    /// found anywhere in the corpus tree) and report pass / fail /
    /// absent (see docket::run).
    Run {
        /// The claim id to run the evaluator for.
        claim: String,
        /// Corpus root — also the marker scan root and each marker
        /// command's working directory.
        #[arg(long, default_value = ".")]
        corpus: PathBuf,
    },
}

fn main() -> ExitCode {
    match Cli::parse().command {
        Command::Check {
            corpus: corpus_root,
            out,
            register,
        } => run_check(&corpus_root, out.as_deref(), &register),
        Command::Blast {
            target,
            corpus: corpus_root,
        } => run_blast(&corpus_root, &target),
        Command::Run {
            claim,
            corpus: corpus_root,
        } => run_run(&corpus_root, &claim),
    }
}

fn run_check(corpus_root: &Path, out: Option<&Path>, register_path: &Path) -> ExitCode {
    let cfg = match config::load_config(corpus_root) {
        Ok(cfg) => cfg,
        Err(e) => return usage_error(&e),
    };

    let loaded = match corpus::load_corpus(corpus_root, &cfg) {
        Ok(loaded) => loaded,
        Err(e) => return usage_error(&e),
    };

    // Resolved relative to the current directory, not --corpus: the
    // register evaluator is a project-level artifact shared across every
    // corpus root (see docket::checks::DEFAULT_REGISTER_RELATIVE_PATH).
    let evaluation = match checks::run_checks(&loaded, &cfg, register_path) {
        Ok(evaluation) => evaluation,
        Err(e) => return usage_error(&e),
    };
    let report = evaluation.report;

    let json = serde_json::to_string_pretty(&evaluation.index).expect("Index serializes");
    match out {
        Some(path) => {
            if let Err(e) = std::fs::write(path, &json) {
                eprintln!("error: could not write {}: {e}", path.display());
                return ExitCode::from(2);
            }
        }
        None => println!("{json}"),
    }

    for diagnostic in &report.diagnostics {
        let severity = match diagnostic.severity {
            checks::Severity::Fail => "error",
            checks::Severity::Warn => "warning",
        };
        eprintln!(
            "{severity}: {}: {}:{}: {}",
            diagnostic.check,
            diagnostic.file,
            diagnostic.line,
            diagnostic.message
        );
    }

    if report.passed() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn run_blast(corpus_root: &Path, target: &str) -> ExitCode {
    let cfg = match config::load_config(corpus_root) {
        Ok(cfg) => cfg,
        Err(e) => return usage_error(&e),
    };

    let loaded = match corpus::load_corpus(corpus_root, &cfg) {
        Ok(loaded) => loaded,
        Err(e) => return usage_error(&e),
    };

    // Dispatch on the argument's shape via the same ref grammar
    // `depends`/`because` entries use (model::CiteRef) — not a second
    // notion of what a ref is. An argument that does not resolve in the
    // corpus is a usage
    // error (exit 2), the same treatment already given an unknown claim
    // id; a ref that resolves but has no citers is a legitimate empty
    // answer (exit 0), so the two must not be conflated.
    match CiteRef::parse(target) {
        CiteRef::Claim(id) => {
            if !loaded.corpus.claims.iter().any(|c| c.id == id) {
                eprintln!("error: no claim with id {id:?} in this corpus");
                return ExitCode::from(2);
            }
        }
        CiteRef::DocAnchor { path, anchor } => {
            let resolves = loaded
                .corpus
                .documents
                .iter()
                .find(|d| d.doc_path == path)
                .is_some_and(|d| d.headings.iter().any(|h| anchor_matches(&h.text, &anchor)));
            if !resolves {
                eprintln!("error: document anchor {target:?} does not resolve in this corpus");
                return ExitCode::from(2);
            }
        }
    }

    let result = blast::blast_radius(&loaded.corpus, target);
    for entry in &result.entries {
        println!("{}\t{}", entry.claim, entry.file);
    }
    for cycle in &result.cycles {
        eprintln!(
            "cycle: {} cites back to {}, not followed again",
            cycle.claim, cycle.ancestor
        );
    }

    ExitCode::SUCCESS
}

fn run_run(corpus_root: &Path, claim_id: &str) -> ExitCode {
    let cfg = match config::load_config(corpus_root) {
        Ok(cfg) => cfg,
        Err(e) => return usage_error(&e),
    };

    let loaded = match corpus::load_corpus(corpus_root, &cfg) {
        Ok(loaded) => loaded,
        Err(e) => return usage_error(&e),
    };

    let markers = match marker::scan_markers(corpus_root) {
        Ok(markers) => markers,
        Err(e) => return usage_error(&e),
    };

    let result = match run::run_claim(&loaded.corpus, claim_id, corpus_root, &markers) {
        Ok(result) => result,
        Err(RunError::UnknownClaim(id)) => {
            eprintln!("error: no claim with id {id:?} in this corpus");
            return ExitCode::from(2);
        }
        Err(e @ RunError::Spawn(_)) => return usage_error(&e),
    };

    print_run_result(&result);

    // 0 pass/none, 1 fail, 2 usage error (above), 3 absent, 4 vacuous —
    // see run.rs's module docs for why each gets its own code rather
    // than sharing another's: a reader gating CI on this exit code can
    // tell "write the marker" from "the evaluator regressed" from "the
    // evaluator ran but checked nothing" without parsing stdout.
    match result.outcome {
        Outcome::Pass | Outcome::None => ExitCode::SUCCESS,
        Outcome::Fail => ExitCode::FAILURE,
        Outcome::Absent => ExitCode::from(3),
        Outcome::Vacuous => ExitCode::from(4),
    }
}

fn print_run_result(result: &RunResult) {
    println!(
        "{}\t{}\t{}",
        result.outcome.as_str(),
        result.claim_id,
        result.evaluator
    );

    if result.outcome == Outcome::Absent {
        println!(
            "  no `docket: {} :: <command>` marker found under the corpus",
            result.claim_id
        );
        return;
    }

    for m in &result.markers {
        let status = if let Some(signal) = m.vacuous {
            format!("VACUOUS ({signal})")
        } else if m.success {
            "ok".to_string()
        } else {
            match m.exit_code {
                Some(code) => format!("FAIL (exit {code})"),
                None => "FAIL (killed by signal)".to_string(),
            }
        };
        println!(
            "  {status}\t{}:{}\t{}",
            m.marker.file, m.marker.line, m.marker.command
        );
        // Terse on genuine success — the command and its exit status
        // already say everything a passing marker needs to; captured
        // output earns its keep only when there's something to
        // diagnose, which a vacuous "success" is exactly as much as a
        // failure.
        if !m.success || m.vacuous.is_some() {
            if !m.stdout.is_empty() {
                println!("  --- stdout ---");
                for line in m.stdout.lines() {
                    println!("  {line}");
                }
            }
            if !m.stderr.is_empty() {
                println!("  --- stderr ---");
                for line in m.stderr.lines() {
                    println!("  {line}");
                }
            }
        }
    }
}

fn usage_error(e: &dyn std::error::Error) -> ExitCode {
    eprintln!("error: {e}");
    ExitCode::from(2)
}
