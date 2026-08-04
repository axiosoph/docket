//! Thin process-invocation wrapper around the `nickel` CLI.
//!
//! MVP.md §7: "Nickel is required for the contract and `docket.ncl`.
//! Validation should invoke Nickel rather than reimplementing its
//! checking." Every place this crate needs Nickel semantics — reading
//! `docket.ncl`, or checking a claim block against the contract (C1) —
//! goes through here rather than growing its own YAML/Nickel-shaped
//! validation logic.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[derive(Debug, thiserror::Error)]
pub enum NickelError {
    #[error("could not run `nickel` (is it on PATH?): {0}")]
    Spawn(#[source] std::io::Error),
    #[error("could not write nickel's input: {0}")]
    Io(#[source] std::io::Error),
    #[error("nickel produced output that wasn't valid JSON: {0}")]
    InvalidJson(#[source] serde_json::Error),
    /// `nickel export` exited non-zero — either the file doesn't evaluate
    /// (syntax error, missing import, …) or, when a contract was applied,
    /// the value doesn't satisfy it. Nickel's own diagnostic (stderr)
    /// already names the file, line, and offending value, so it is
    /// surfaced near-verbatim rather than re-derived — only its leading
    /// `error: ` is stripped, since every caller wraps this in its own
    /// `error: {e}` (main.rs's `usage_error`) and the two would otherwise
    /// double up.
    #[error("{0}")]
    Failed(String),
}

/// Evaluate `nickel_path` and validate it against `contract_path` in one
/// step, returning the validated value as JSON.
///
/// Used to read `docket.ncl`: `contracts/docket.ncl` is the sole
/// authority for the config's shape (genre `path`/`kinds`/`quadrant`, the
/// `Kind`/`Quadrant` enums, and the explanation-forbids-kinds derived
/// rule) — callers must not re-derive any of it, only convert the
/// already-validated JSON into typed values.
pub fn export_json_with_contract(
    nickel_path: &Path,
    contract_path: &Path,
) -> Result<serde_json::Value, NickelError> {
    let output = Command::new("nickel")
        .arg("export")
        .arg("--format")
        .arg("json")
        .arg(nickel_path)
        .arg("--apply-contract")
        .arg(contract_path)
        .output()
        .map_err(NickelError::Spawn)?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let diagnostic = stderr.strip_prefix("error: ").unwrap_or(&stderr);
        return Err(NickelError::Failed(diagnostic.to_string()));
    }

    serde_json::from_slice(&output.stdout).map_err(NickelError::InvalidJson)
}

/// Evaluate `register_path`'s `evaluate` function against `input_json` —
/// the register evaluator (`.ledger/2026-07-30-reference-kinds-and-document-resolution.md`,
/// R6): one Nickel evaluation that computes the index and every check's
/// diagnostics over an already-extracted corpus, replacing what was N
/// per-claim `nickel export` invocations (C1) plus a Rust-native pass for
/// every other check.
///
/// `input_json` is written to a temp file (Nickel's `import` needs a real
/// path); the driver expression that imports both the data and
/// `register.ncl` and applies one to the other is piped over stdin, the
/// same mechanism this module already uses for a contract check.
pub fn evaluate_register(
    register_path: &Path,
    input_json: &str,
) -> Result<serde_json::Value, NickelError> {
    let data_path = write_temp_json(input_json).map_err(NickelError::Io)?;
    let result = (|| {
        // Absolute paths only: the driver expression arrives over stdin,
        // not as a file, so Nickel has no directory to resolve a
        // relative `import` against ("looked in []" is the diagnostic
        // when this is gotten wrong).
        let register_abs = std::path::absolute(register_path).map_err(NickelError::Io)?;
        let driver = format!(
            "(import \"{register}\").evaluate (import \"{data}\")",
            register = escape_nickel_string_literal(&register_abs.to_string_lossy()),
            data = escape_nickel_string_literal(&data_path.to_string_lossy()),
        );

        let mut child = Command::new("nickel")
            .arg("export")
            .arg("--format")
            .arg("json")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(NickelError::Spawn)?;

        child
            .stdin
            .take()
            .expect("stdin was piped")
            .write_all(driver.as_bytes())
            .map_err(NickelError::Io)?;

        let output = child.wait_with_output().map_err(NickelError::Spawn)?;

        if output.status.success() {
            serde_json::from_slice(&output.stdout).map_err(NickelError::InvalidJson)
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let diagnostic = stderr.strip_prefix("error: ").unwrap_or(&stderr);
            Err(NickelError::Failed(diagnostic.to_string()))
        }
    })();

    let _ = std::fs::remove_file(&data_path);
    result
}

fn escape_nickel_string_literal(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn write_temp_json(content: &str) -> std::io::Result<PathBuf> {
    let unique = format!(
        "docket-{}-{}.json",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default()
    );
    let path = std::env::temp_dir().join(unique);
    std::fs::write(&path, content)?;
    Ok(path)
}
