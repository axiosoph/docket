//! C1: claim block validation against the Nickel contract (MVP.md §1.2,
//! §3, §7: "Nickel is required for the contract... Validation should
//! invoke Nickel rather than reimplementing its checking.").

use crate::nickel::{self, ContractCheck, NickelError};
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum ContractError {
    #[error(transparent)]
    Nickel(#[from] NickelError),
}

/// Where the claim-block contract is expected to live, relative to the
/// corpus root.
///
/// **This is a documented assumption, not a spec fact.** Neither
/// README.md ("validated by a committed Nickel contract") nor MVP.md §7
/// ("Nickel is required for the contract") names a path or export shape —
/// that file is a sibling deliverable (`contracts/claim.ncl`, owned by a
/// parallel worker, per this crate's own dispatch), and this crate can't
/// pin the integration seam down unilaterally. It's a single constant,
/// trivially overridden via `docket check --contract <path>`; flagged in
/// the implementation report as needing confirmation once the contract
/// lands.
pub const DEFAULT_CONTRACT_RELATIVE_PATH: &str = "contracts/claim.ncl";

/// Validate one claim block's raw YAML (everything it declared, unknown
/// fields included — that's the point of C1) against the Nickel contract
/// at `contract_path`.
///
/// A YAML document that doesn't even parse is itself a C1 violation
/// (MVP.md §3: "malformed or unknown field"), reported the same way a
/// contract-rejected value is — not surfaced as a distinct Rust error,
/// since from a caller's point of view both mean "this block does not
/// validate."
pub fn validate_claim_block(contract_path: &Path, raw_yaml: &str) -> Result<ContractCheck, ContractError> {
    let value: serde_norway::Value = match serde_norway::from_str(raw_yaml) {
        Ok(v) => v,
        Err(e) => {
            return Ok(ContractCheck::Violated {
                diagnostic: format!("invalid YAML: {e}"),
            });
        }
    };
    // serde_norway::Value -> JSON: both are serde Value types over the
    // same data model (mappings, sequences, scalars), so this is a
    // straight re-serialization, not a lossy conversion for the shapes
    // MVP.md's claim block permits (records, arrays, strings).
    let json = serde_json::to_string(&value).expect("a parsed YAML value always re-serializes to JSON");
    Ok(nickel::check_contract(contract_path, &json)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// A small, self-contained contract mirroring MVP.md §1.2's field
    /// table, written to a temp file. This crate doesn't own the real
    /// `contracts/claim.ncl` (a parallel worker does, and it may not
    /// exist yet) — this fixture exists only to prove the invocation
    /// mechanics in `nickel.rs` work end-to-end against *a* Nickel
    /// contract, independent of the real one's exact content.
    const TEST_CONTRACT: &str = r#"
{
  kind | std.contract.from_predicate (fun v => std.array.elem v ["requirement", "invariant", "constraint"]),
  evaluator | std.contract.from_predicate (fun v => std.array.elem v ["proof", "model-check", "property-test", "test", "example", "none"]),
  cites | Array String | default = [],
}
"#;

    fn write_test_contract() -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "docket-contract-test-{}-{}.ncl",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(TEST_CONTRACT.as_bytes()).unwrap();
        path
    }

    #[test]
    fn a_well_formed_claim_block_validates() {
        let contract = write_test_contract();
        let yaml = "kind: constraint\nevaluator: property-test\ncites: [composition-model#6]\n";
        let result = validate_claim_block(&contract, yaml).unwrap();
        assert!(matches!(result, ContractCheck::Valid));
        let _ = std::fs::remove_file(contract);
    }

    #[test]
    fn missing_cites_defaults_to_empty_and_still_validates() {
        let contract = write_test_contract();
        let yaml = "kind: invariant\nevaluator: none\n";
        let result = validate_claim_block(&contract, yaml).unwrap();
        assert!(matches!(result, ContractCheck::Valid));
        let _ = std::fs::remove_file(contract);
    }

    #[test]
    fn an_unknown_evaluator_is_a_violation() {
        let contract = write_test_contract();
        let yaml = "kind: constraint\nevaluator: vibes\n";
        let result = validate_claim_block(&contract, yaml).unwrap();
        match result {
            ContractCheck::Violated { diagnostic } => assert!(!diagnostic.is_empty()),
            ContractCheck::Valid => panic!("expected a violation for an unrecognized evaluator"),
        }
        let _ = std::fs::remove_file(contract);
    }

    #[test]
    fn an_unknown_field_is_a_violation() {
        // MVP.md §1.2: "Unknown fields are an error, not ignored."
        let contract = write_test_contract();
        let yaml = "kind: constraint\nevaluator: test\ntypo_field: oops\n";
        let result = validate_claim_block(&contract, yaml).unwrap();
        assert!(matches!(result, ContractCheck::Violated { .. }));
        let _ = std::fs::remove_file(contract);
    }

    #[test]
    fn invalid_yaml_is_reported_as_a_violation_not_a_rust_error() {
        let contract = write_test_contract();
        let yaml = "kind: [unterminated\n";
        let result = validate_claim_block(&contract, yaml).unwrap();
        assert!(matches!(result, ContractCheck::Violated { .. }));
        let _ = std::fs::remove_file(contract);
    }
}
