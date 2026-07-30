# Apply-contract shim for claim-block YAML instances.
#
# Usage (from the project root, with nickel on PATH):
#   nickel export <file>.yaml --apply-contract contracts/claim_apply.ncl
#
# A claim block's content is pure-data YAML (no Nickel execution model) —
# validated externally by this shim. Self-binding and import-DoS are
# impossible by construction (YAML has no execution model).
#
# Imports resolve relative to THIS file's location (contracts/), so
# claim.ncl is found in the same directory without any -I flag. Shape
# follows predicate's own record-contract shim pattern; see
# /var/home/nrd/git/github.com/nrdxp/predicate/skills/record/process_feedback_apply.ncl
(import "claim.ncl").Claim
