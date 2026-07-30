# Contract for a claim block's YAML payload (MVP.md §1.2).
#
# The `id` is deliberately absent from this contract: MVP.md §1.1 sources
# it from the nearest preceding `[kebab-case]` heading in the same file,
# never duplicating it into the block — one statement, one place.
#
# Instance format: YAML. Validate an extracted claim block via the
# apply-contract shim (claim_apply.ncl):
#   nickel export <file>.yaml --apply-contract contracts/claim_apply.ncl
{
  NonEmptyString = std.contract.from_predicate (fun v =>
    std.is_string v && std.string.length v > 0
  ),

  # kind: what the claim asserts (MVP.md §1.2). Also reused by docket.ncl
  # to validate a genre's permitted `kinds` list, so both files agree on
  # the one set of legal values.
  Kind = std.contract.from_predicate (fun v =>
    v == 'requirement || v == "requirement"
    || v == 'invariant || v == "invariant"
    || v == 'constraint || v == "constraint"
  ),

  # evaluator: how the claim's status would be discharged. The evaluator
  # runner itself is out of MVP scope (README.md "MVP scope") — only the
  # name is recorded and validated here.
  #
  # The values are a PREFERENCE ORDER, not an unordered set (README.md,
  # "The hierarchy is normative, not merely descriptive"):
  #
  #   proof > type > property-test > test > example > none
  #
  # `type` means the claim is enforced by construction — the violation
  # cannot be written, rather than being caught when written. That is a
  # categorically stronger discharge than a test, and where it is
  # elegantly reachable it should be preferred. Its honesty limit: the
  # machine confirms the marked item exists and builds, never that the type
  # enforces what the claim says, so faithfulness is review-established.
  Evaluator = std.contract.from_predicate (fun v =>
    v == 'proof          || v == "proof"
    || v == 'model-check   || v == "model-check"
    || v == 'type          || v == "type"
    || v == 'property-test || v == "property-test"
    || v == 'test          || v == "test"
    || v == 'example       || v == "example"
    || v == 'none          || v == "none"
  ),

  # A `depends`/`because` entry is a claim id or a `<doc-stem>#<anchor>`
  # document reference (MVP.md §1.3). Resolving the target against the
  # corpus is C4's/`orphaned-because`'s job (needs the whole corpus in
  # view), not this contract's — this only checks the two syntactic
  # shapes:
  #
  #   - a bare kebab-case claim id, matching the heading-token grammar
  #     MVP.md §1.1 defines for `[id]` headings.
  #   - a doc-stem#anchor pair. The doc stem is a file basename and is NOT
  #     constrained to kebab-case (e.g. "MVP", "README" are valid stems),
  #     so only the shape — one `#` splitting two non-empty,
  #     whitespace-free halves — is checked.
  ClaimIdPattern = "^[a-z0-9]+(-[a-z0-9]+)*$",
  DocRefPattern = "^[^\\s#]+#[^\\s#]+$",

  Ref = std.contract.from_predicate (fun v =>
    std.is_string v
    && (std.string.is_match ClaimIdPattern v || std.string.is_match DocRefPattern v)
  ),

  # The claim block itself. Unknown fields are a hard error (MVP.md §1.2,
  # C1) — achieved for free by Nickel's closed-by-default record
  # contracts; no `, ..` appears anywhere in this file.
  #
  # Reference kinds (`.ledger/2026-07-30-reference-kinds-and-document-resolution.md`,
  # R1): the single `cites` field is retired in favour of two, because the
  # two kinds carry different consequences when their target is deleted —
  # `depends` means the claim breaks; `because` means only its stated
  # reason is orphaned. There is deliberately no third field for a bare
  # reference: R3 defines bare as "a prose link that is not declared", so
  # it has no declaration to validate — adding a field for it would let an
  # author assert bareness, which contradicts what bareness means.
  #
  # Both default to `[]` (MVP.md §1.2's stated default for the field they
  # replace), applied here so every validated instance carries them.
  Claim = {
    kind      | Kind,
    evaluator | Evaluator,
    depends
      | Array Ref
      | optional
      | default
      = [],
    because
      | Array Ref
      | optional
      | default
      = [],
  },
}
