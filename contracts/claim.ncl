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
  #
  # The predicate is exposed bare (`KindPred`), not only wrapped as a
  # contract, because the register evaluator (`register.ncl`) needs to
  # test every claim in a corpus and collect *every* violation — a
  # contract application aborts evaluation at the first one (confirmed
  # directly: `Array Claim | Contract` on a batch surfaces exactly one
  # failing element, not all of them), which would silently under-report
  # C1 relative to the one-nickel-process-per-claim design this replaces.
  # `Kind` stays a real contract for single-claim usage (`claim_apply.ncl`,
  # ad-hoc `nickel export --apply-contract`) — one predicate, two
  # consumption modes, not two sources of truth.
  KindPred = fun v =>
    v == 'requirement || v == "requirement"
    || v == 'invariant || v == "invariant"
    || v == 'constraint || v == "constraint",
  Kind = std.contract.from_predicate KindPred,

  # evaluator: how the claim's status would be discharged. The evaluator
  # runner itself is out of MVP scope (README.md "MVP scope") — only the
  # name is recorded and validated here.
  #
  # The mechanical values are a PREFERENCE ORDER, not an unordered set
  # (README.md, "The hierarchy is normative, not merely descriptive"):
  #
  #   proof > model-check > type > property-test > test > example > none
  #
  # `type` means the claim is enforced by construction — the violation
  # cannot be written, rather than being caught when written. That is a
  # categorically stronger discharge than a test, and where it is
  # elegantly reachable it should be preferred. Its honesty limit: the
  # machine confirms the marked item exists and builds, never that the type
  # enforces what the claim says, so faithfulness is review-established.
  #
  # `review` sits OUTSIDE that order — never insert it as a weaker rung
  # below `none` or anywhere else in the chain. It means a human or agent
  # read the claim against its target and it holds: an assertion that
  # the claim is TRUE, backed by
  # testimony rather than a re-runnable check — a **vouch**, the
  # register's other evidence species, not a corroboration one grade
  # short of `example`. `none` asserts the opposite: nothing has been
  # done. The two are not comparable on the mechanical scale at all.
  #
  # `absent` also sits OUTSIDE the order, for a different reason than
  # `review`'s: it IS a corroboration (a re-runnable, decorrelated
  # check, like every mechanical grade above it), but the hierarchy
  # ranks strength of evidence for a claim that something holds —
  # proof, type, property test, example test each demonstrate a
  # positive. An `absent` claim demonstrates the opposite predicate: a
  # named literal does NOT occur in the corpus's non-documentation
  # source — absence is a claim, and it is the one that inverts. Ranking
  # it against `test` or `example` would compare unlike axes; see
  # `run.rs`'s module docs for the mechanism (it reuses the SAME
  # `@docket:` marker grammar every other mechanical evaluator uses —
  # for `absent`, the marker's command field is the literal to search
  # for, not a command to execute).
  EvaluatorPred = fun v =>
    v == 'proof          || v == "proof"
    || v == 'model-check   || v == "model-check"
    || v == 'type          || v == "type"
    || v == 'property-test || v == "property-test"
    || v == 'test          || v == "test"
    || v == 'example       || v == "example"
    || v == 'absent        || v == "absent"
    || v == 'review        || v == "review"
    || v == 'none          || v == "none",
  Evaluator = std.contract.from_predicate EvaluatorPred,

  # verification: the SAME evaluator vocabulary above, now keyed by WHAT
  # it is evidence about — `design` (the specification: does the model
  # cohere) or `implementation` (the code: does it behave per the
  # design) — rather than only how strong the evidence is. Orthogonal to
  # `evaluator`, not a replacement for it: a claim declares exactly one
  # of the two (`ClaimTargetValid`, below), the same "one statement, one
  # place" principle this file already states for `id`.
  #
  # A key PRESENT means the claim has that target; a key ABSENT means it
  # doesn't. A key present with value `none` means the target EXISTS and
  # nothing checks it yet — the honest gap `evaluator: none` already
  # names, now locatable to an axis. That distinction (absent vs. `none`)
  # is exactly what one field could not carry, and is the reason this
  # axis exists at all: measured against a real corpus, 15 of 22 claims
  # graded `model-check`/`review` were implementation-pending — reading
  # as covered in a metric meant to track implementation progress
  # (`.ledger/state/verification-targets.md`, the consumer project this
  # contract serves).
  KnownVerificationFields = ["design", "implementation"],

  VerificationValid = std.contract.from_validator (fun v =>
    if !std.is_record v then
      'Error { message = "`verification` must be a mapping of `design`/`implementation` to an evaluator value" }
    else
      let fields = std.record.fields v in
      if std.array.length fields == 0 then
        'Error { message = "`verification` must declare at least one of `design`, `implementation` — an empty mapping is a schema error, not a gap" }
      else
        let unknown = std.array.filter (fun f => !std.array.elem f KnownVerificationFields) fields in
        if std.array.length unknown > 0 then
          'Error { message = "verification field(s) not permitted: %{std.string.join ", " unknown} (only `design`/`implementation`)" }
        else if std.array.elem "design" fields && !EvaluatorPred v.design then
          'Error { message = "field `verification.design` has an invalid value" }
        else if std.array.elem "implementation" fields && !EvaluatorPred v.implementation then
          'Error { message = "field `verification.implementation` has an invalid value" }
        else
          'Ok
  ),

  # A claim states its verification target exactly once: `evaluator`
  # (legacy, still accepted corpus-wide) or `verification` (this axis),
  # never both — the same "one statement, one place" principle this file
  # already states about `id`, applied to a second field pair. Enforced
  # here so single-claim usage (`claim_apply.ncl`) rejects it identically
  # to the batch validator (`register.ncl`'s `claim_field_violations`,
  # which re-derives this rule rather than calling this contract — see
  # that file's own header comment for why an array of claims can't
  # apply a contract without losing every violation but the first).
  #
  # Takes the record to check as its argument (not the record itself,
  # merged with something else via `|`) — a contract built by merging
  # `SomeShape | XorCheck` forces `SomeShape` *eagerly*, against no real
  # instance at all, since `|` always validates its left-hand side
  # immediately, before any later merge (confirmed directly: composing
  # two contracts this way rejected a valid instance with this rule's
  # own "missing" message, applied to the bare schema rather than the
  # instance being checked — Nickel 1.17.0). `Claim` (below) instead
  # calls this as a function on its own argument, deferred until an
  # actual instance is checked against `Claim`.
  claim_target_ok = fun c =>
    let has_evaluator = std.record.has_field "evaluator" c in
    let has_verification = std.record.has_field "verification" c in
    if has_evaluator && has_verification then
      'Error { message = "fields `evaluator` and `verification` are mutually exclusive — one statement, one place" }
    else if !has_evaluator && !has_verification then
      'Error { message = "missing required field(s): evaluator or verification" }
    else
      'Ok,

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

  RefPred = fun v =>
    std.is_string v
    && (std.string.is_match ClaimIdPattern v || std.string.is_match DocRefPattern v),
  Ref = std.contract.from_predicate RefPred,

  # The field vocabulary a claim block may declare, and which of those are
  # required — the data `register.ncl`'s batch C1 validator needs to
  # detect an unknown field and a missing required one without Nickel's
  # closed-record machinery (which reports a violation, not a field list).
  # Kept beside `Claim` so the two can't silently drift: adding a field to
  # one without the other is now a visible omission, not a latent one.
  KnownClaimFields = ["kind", "evaluator", "verification", "depends", "because"],
  # `evaluator` is deliberately absent from this list now: a claim is
  # required to state a verification target, but it may do so via
  # `evaluator` (legacy) or `verification` (this axis) — `kind` is the
  # only field required unconditionally. `ClaimTargetValid` (above)
  # enforces the "exactly one of the two" rule this list can't express.
  RequiredClaimFields = ["kind"],

  # The claim block itself. Unknown fields are a hard error (MVP.md §1.2,
  # C1) — achieved for free by Nickel's closed-by-default record
  # contracts; no `, ..` appears anywhere in this file.
  #
  # Reference kinds (R1): the single `cites` field is retired in favour
  # of two, because the two kinds carry different consequences when
  # their target is deleted —
  # `depends` means the claim breaks; `because` means only its stated
  # reason is orphaned. There is deliberately no third field for a bare
  # reference: R3 defines bare as "a prose link that is not declared", so
  # it has no declaration to validate — adding a field for it would let an
  # author assert bareness, which contradicts what bareness means.
  #
  # Both default to `[]` (MVP.md §1.2's stated default for the field they
  # replace), applied here so every validated instance carries them.
  ClaimShape = {
    kind          | Kind,
    evaluator     | Evaluator | optional,
    verification  | VerificationValid | optional,
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

  # A genuine contract whose function receives the actual instance being
  # checked as its argument — see `claim_target_ok`'s comment for why
  # that indirection is load-bearing rather than stylistic. `checked` is
  # `value` after `ClaimShape`'s per-field contracts (and `depends`'/
  # `because`'s defaults) have already applied, so `claim_target_ok` sees
  # exactly what a reader of the merged record would.
  #
  # `std.contract.custom`, not `std.contract.from_validator`, because
  # `from_validator`'s successful branch discards whatever the validator
  # function computed and returns the *original*, unmerged `value` — so
  # `ClaimShape`'s per-field contracts (Kind, Evaluator, VerificationValid,
  # Ref, the `depends`/`because` defaults) would silently never attach to
  # anything downstream ever sees (confirmed directly: `checked` computed
  # this way and then discarded let an empty `verification: {}` and an
  # unrecognized `evaluator` value both through — Nickel 1.17.0).
  # `std.contract.custom`'s `'Ok value` form returns `checked` itself as
  # the validated result, so every nested contract this composes stays
  # attached.
  Claim = std.contract.custom (fun label value =>
    let checked = value | ClaimShape in
    let result = claim_target_ok checked in
    if result == 'Ok then 'Ok checked else result
  ),
}
