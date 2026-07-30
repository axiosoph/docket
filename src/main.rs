//! A thin CLI over the `docket` library. The checks are pure functions
//! over parsed input (MVP.md §7); this binary is just argument parsing,
//! I/O, and exit codes (§5).

use clap::{Parser, Subcommand};
use docket::model::{CiteRef, anchor_matches};
use docket::{blast, checks, config, contract, corpus, index};
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
    /// Run the five checks (+ orphan-claim) and emit the index.
    Check {
        /// Corpus root.
        #[arg(long, default_value = ".")]
        corpus: PathBuf,
        /// Write the index JSON here instead of stdout.
        #[arg(long)]
        out: Option<PathBuf>,
        /// Path to the claim-block Nickel contract's apply shim, resolved
        /// relative to the current directory (it's a project-level
        /// artifact, not per-corpus — see docket::contract).
        #[arg(long, default_value = contract::DEFAULT_CONTRACT_RELATIVE_PATH)]
        contract: PathBuf,
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
}

fn main() -> ExitCode {
    match Cli::parse().command {
        Command::Check {
            corpus: corpus_root,
            out,
            contract,
        } => run_check(&corpus_root, out.as_deref(), &contract),
        Command::Blast {
            target,
            corpus: corpus_root,
        } => run_blast(&corpus_root, &target),
    }
}

fn run_check(corpus_root: &Path, out: Option<&Path>, contract_path: &Path) -> ExitCode {
    let cfg = match config::load_config(corpus_root) {
        Ok(cfg) => cfg,
        Err(e) => return usage_error(&e),
    };

    let loaded = match corpus::load_corpus(corpus_root, &cfg) {
        Ok(loaded) => loaded,
        Err(e) => return usage_error(&e),
    };

    // Resolved relative to the current directory, not --corpus: the
    // contract is a project-level artifact shared across every corpus
    // root (see docket::contract::DEFAULT_CONTRACT_RELATIVE_PATH).
    let report = match checks::run_checks(&loaded, &cfg, contract_path) {
        Ok(report) => report,
        Err(e) => return usage_error(&e),
    };

    let idx = index::build_index(&loaded.corpus);
    let json = serde_json::to_string_pretty(&idx).expect("Index serializes");
    match out {
        Some(path) => {
            if let Err(e) = std::fs::write(path, &json) {
                eprintln!("error: could not write {}: {e}", path.display());
                return ExitCode::from(2);
            }
        }
        None => println!("{json}"),
    }

    for failure in &report.failures {
        eprintln!(
            "{}: {}:{}: {}",
            failure.check.as_str(),
            failure.file,
            failure.line,
            failure.message
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

    // Dispatch on the argument's shape via the same ref grammar `cites`
    // entries use (model::CiteRef) — not a second notion of what a ref
    // is. An argument that does not resolve in the corpus is a usage
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

fn usage_error(e: &dyn std::error::Error) -> ExitCode {
    eprintln!("error: {e}");
    ExitCode::from(2)
}
