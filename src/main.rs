//! A thin CLI over the `docket` library. The checks are pure functions
//! over parsed input (MVP.md §7); this binary is just argument parsing,
//! I/O, and exit codes (§5).

use clap::{Parser, Subcommand};
use docket::{blast, checks, config, contract, corpus, index};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

#[derive(Parser)]
#[command(name = "docket", about = "A register of claims across a documentation corpus.")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run the five checks (+ orphan-claim, duplicate-stem) and emit the index.
    Check {
        /// Corpus root.
        #[arg(long, default_value = ".")]
        corpus: PathBuf,
        /// Write the index JSON here instead of stdout.
        #[arg(long)]
        out: Option<PathBuf>,
        /// Path to the claim-block Nickel contract, resolved relative to
        /// `--corpus`. See docket::contract for why this default is a
        /// documented assumption rather than a spec fact.
        #[arg(long, default_value = contract::DEFAULT_CONTRACT_RELATIVE_PATH)]
        contract: PathBuf,
    },
    /// Print the transitive blast radius of a claim id (§4.2).
    Blast {
        claim_id: String,
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
            claim_id,
            corpus: corpus_root,
        } => run_blast(&corpus_root, &claim_id),
    }
}

fn run_check(corpus_root: &Path, out: Option<&Path>, contract_relative: &Path) -> ExitCode {
    let cfg = match config::load_config(corpus_root) {
        Ok(cfg) => cfg,
        Err(e) => return usage_error(&e),
    };

    let loaded = match corpus::load_corpus(corpus_root, &cfg) {
        Ok(loaded) => loaded,
        Err(e) => return usage_error(&e),
    };

    let contract_path = corpus_root.join(contract_relative);
    let report = match checks::run_checks(&loaded, &cfg, &contract_path) {
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
        match failure.line {
            Some(line) => eprintln!(
                "{}: {}:{}: {}",
                failure.check.as_str(),
                failure.file,
                line,
                failure.message
            ),
            None => eprintln!("{}: {}: {}", failure.check.as_str(), failure.file, failure.message),
        }
    }

    if report.passed() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn run_blast(corpus_root: &Path, claim_id: &str) -> ExitCode {
    let cfg = match config::load_config(corpus_root) {
        Ok(cfg) => cfg,
        Err(e) => return usage_error(&e),
    };

    let loaded = match corpus::load_corpus(corpus_root, &cfg) {
        Ok(loaded) => loaded,
        Err(e) => return usage_error(&e),
    };

    if !loaded.corpus.claims.iter().any(|c| c.id == claim_id) {
        eprintln!("error: no claim with id {claim_id:?} in this corpus");
        return ExitCode::from(2);
    }

    let result = blast::blast_radius(&loaded.corpus, claim_id);
    for entry in &result.entries {
        println!("{}\t{}", entry.claim, entry.file);
    }
    for cycle in &result.cycles {
        eprintln!("cycle: {} cites back to {}, not followed again", cycle.claim, cycle.ancestor);
    }

    ExitCode::SUCCESS
}

fn usage_error(e: &dyn std::error::Error) -> ExitCode {
    eprintln!("error: {e}");
    ExitCode::from(2)
}
