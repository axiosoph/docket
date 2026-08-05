//! A thin CLI over the `docket` library. The checks are pure functions
//! over parsed input (MVP.md §7); this binary is just argument parsing,
//! I/O, and exit codes (§5).

use clap::{Parser, Subcommand};
use docket::model::{CiteRef, anchor_matches};
use docket::run::{Outcome, RunError, RunResult};
use docket::signals::GraphSignals;
use docket::{blast, checks, config, contracts, corpus, marker, run, signals};
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
        /// Path to the register evaluator. It ships with docket, not with
        /// the corpus being checked, so the default is docket's own
        /// embedded copy rather than anything resolved against the
        /// current directory. Pass this to use a different evaluator
        /// (e.g. while developing docket itself); it is resolved
        /// relative to the current directory, same as any other path
        /// argument.
        #[arg(long)]
        register: Option<PathBuf>,
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
    /// Report derived properties of the reference graph the register
    /// already carries — nothing declared, nothing new to check against
    /// (see docket::signals). Never fails: every signal here is a
    /// candidate for a reader to weigh, not a verdict.
    Signals {
        /// Corpus root.
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
        } => run_check(&corpus_root, out.as_deref(), register.as_deref()),
        Command::Blast {
            target,
            corpus: corpus_root,
        } => run_blast(&corpus_root, &target),
        Command::Run {
            claim,
            corpus: corpus_root,
        } => run_run(&corpus_root, &claim),
        Command::Signals {
            corpus: corpus_root,
        } => run_signals(&corpus_root),
    }
}

fn run_check(corpus_root: &Path, out: Option<&Path>, register_override: Option<&Path>) -> ExitCode {
    let cfg = match config::load_config(corpus_root) {
        Ok(cfg) => cfg,
        Err(e) => return usage_error(&e),
    };

    let loaded = match corpus::load_corpus(corpus_root, &cfg) {
        Ok(loaded) => loaded,
        Err(e) => return usage_error(&e),
    };

    // The register evaluator ships with docket, not with --corpus: an
    // explicit --register (resolved relative to the current directory,
    // like any other path argument) always wins; absent that, fall back
    // to docket's own embedded copy rather than anything path-guessed.
    // `materialized_guard` is only initialized on the fallback branch —
    // deliberately: `register_path` must not outlive it, since the
    // materialized directory is removed on drop.
    let materialized_guard;
    let register_path: PathBuf = match register_override {
        Some(p) => p.to_path_buf(),
        None => {
            materialized_guard = match contracts::MaterializedContracts::new() {
                Ok(m) => m,
                Err(e) => return usage_error(&e),
            };
            materialized_guard.register_path()
        }
    };

    let evaluation = match checks::run_checks(&loaded, &cfg, &register_path) {
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
            diagnostic.check, diagnostic.file, diagnostic.line, diagnostic.message
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

    // 0 pass/none/review, 1 fail, 2 usage error (above), 3 absent, 4
    // vacuous — see run.rs's module docs for why each gets its own code
    // rather than sharing another's: a reader gating CI on this exit
    // code can tell "write the marker" from "the evaluator regressed"
    // from "the evaluator ran but checked nothing" without parsing
    // stdout. `Review` shares `Pass`/`None`'s code rather than getting
    // its own: like a pass, it is a claim CI should treat as discharged
    // and not block on — the distinction from `None` (asserted true vs.
    // asserted absent) is a claim about the world, not about whether CI
    // should gate on it, so it is carried in the outcome text
    // (`print_run_result`), not the exit code.
    match result.outcome {
        Outcome::Pass | Outcome::None | Outcome::Review => ExitCode::SUCCESS,
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

fn run_signals(corpus_root: &Path) -> ExitCode {
    let cfg = match config::load_config(corpus_root) {
        Ok(cfg) => cfg,
        Err(e) => return usage_error(&e),
    };

    let loaded = match corpus::load_corpus(corpus_root, &cfg) {
        Ok(loaded) => loaded,
        Err(e) => return usage_error(&e),
    };

    print_signals(&signals::compute(&loaded.corpus));

    // Every signal here is a candidate for a reader to weigh, never a
    // check result — nothing in this report can fail a run the way
    // `check`'s diagnostics do (this command's whole non-negotiable is
    // that it changes no existing exit code or diagnostic).
    ExitCode::SUCCESS
}

fn print_signals(signals: &GraphSignals) {
    let zero_inbound: Vec<_> = signals.zero_inbound().collect();
    println!(
        "{} claims, {} zero-inbound",
        signals.claims.len(),
        zero_inbound.len()
    );
    println!();
    println!(
        "Zero-inbound: no `depends` or `because` entry anywhere in the corpus\n\
         names these claims. That makes each one a CANDIDATE for\n\
         superseded-and-unnoticed — not a verdict. A claim nothing points at\n\
         can be exactly right on its own: a self-contained safety property or\n\
         a forbidden state needs nothing to depend on it, and a healthy\n\
         corpus is expected to carry real leaves like these. This list bounds\n\
         where to look; it does not decide what you'll find there. Read each\n\
         one, confirm it still holds or retire it — dismissing a leaf as\n\
         legitimate is exactly as much a use of this report as fixing one\n\
         isn't."
    );
    println!();
    for c in &zero_inbound {
        println!("  {}\t{}:{}", c.id, c.file, c.line);
    }
    println!();
    println!(
        "Per-claim graph position — out-degree (refs this claim declares),\n\
         in-degree (refs naming it), review-surface (this claim's blast\n\
         radius: how many claims would need re-checking, transitively, if it\n\
         were removed — see `docket blast`):"
    );
    println!();
    println!("id\tfile\tline\tout-degree\tin-degree\treview-surface");
    for c in &signals.claims {
        println!(
            "{}\t{}\t{}\t{}\t{}\t{}",
            c.id, c.file, c.line, c.out_degree, c.in_degree, c.review_surface
        );
    }
}

fn usage_error(e: &dyn std::error::Error) -> ExitCode {
    eprintln!("error: {e}");
    ExitCode::from(2)
}
