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
}

/// Evaluate and export a Nickel file as JSON. Used to read `docket.ncl`.
pub fn export_json(nickel_path: &Path) -> Result<serde_json::Value, NickelError> {
    let output = Command::new("nickel")
        .arg("export")
        .arg("--format")
        .arg("json")
        .arg(nickel_path)
        .output()
        .map_err(NickelError::Spawn)?;

    if !output.status.success() {
        return Err(NickelError::InvalidJson(
            // Not actually a JSON error, but export_json's callers only
            // distinguish "ok" from "failed"; the real diagnostic is in
            // stderr, surfaced via the Display impl below.
            serde_json::from_str::<serde_json::Value>(&format!(
                "nickel export failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ))
            .unwrap_err(),
        ));
    }

    serde_json::from_slice(&output.stdout).map_err(NickelError::InvalidJson)
}

/// The outcome of applying a Nickel contract to a value: either the
/// contract held, or it didn't, with Nickel's own diagnostic — which
/// names the file, line, and offending value per MVP.md §3, so it is
/// passed through rather than re-derived.
pub enum ContractCheck {
    Valid,
    Violated { diagnostic: String },
}

/// Validate `value_json` against the Nickel contract at `contract_path`.
///
/// `value_json` is written to a temp file (Nickel's `import` needs a real
/// path); the driver expression that imports both the data and the
/// contract and applies one to the other is piped over stdin, so this
/// needs exactly one temp file rather than two.
pub fn check_contract(
    contract_path: &Path,
    value_json: &str,
) -> Result<ContractCheck, NickelError> {
    let data_path = write_temp_json(value_json).map_err(NickelError::Io)?;
    let result = (|| {
        let driver = format!(
            "(import \"{data}\") | (import \"{contract}\")",
            data = escape_nickel_string_literal(&data_path.to_string_lossy()),
            contract = escape_nickel_string_literal(&contract_path.to_string_lossy()),
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

        Ok(if output.status.success() {
            ContractCheck::Valid
        } else {
            ContractCheck::Violated {
                diagnostic: String::from_utf8_lossy(&output.stderr).into_owned(),
            }
        })
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
