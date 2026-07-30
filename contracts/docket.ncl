# Contract for a repository's `docket.ncl` genre configuration (MVP.md §2).
#
# Unlike a claim block, a `docket.ncl` config is already plain Nickel, not
# an external data format — so it needs no apply-contract shim. Validate a
# repository's config directly:
#   nickel export <path>/docket.ncl --apply-contract contracts/docket.ncl
let NonEmptyString = std.contract.from_predicate (fun v =>
  std.is_string v && std.string.length v > 0
) in

# Reuse claim.ncl's Kind enum rather than restating it — a genre's
# permitted `kinds` are drawn from the same three claim kinds a claim
# block may declare. Restating it here would be exactly the duplication
# surface this tool exists to remove.
let Kind = (import "claim.ncl").Kind in

# Same reuse discipline for `quadrant`: its canonical home is
# quadrant.ncl, imported rather than restated here.
let Quadrant = (import "quadrant.ncl").Quadrant in

# `kinds = []` is legal and means the genre permits no claim blocks at
# all (MVP.md §2) — the mechanism by which a decision record is prevented
# from carrying normative content.
#
# `quadrant` is required, not optional: an optional field would let a
# corpus leave genres unlabelled and have the gap analysis silently
# under-report — three unlabelled genres would read as three empty
# quadrants, indistinguishable from a corpus that genuinely has no
# tutorial/how-to/reference/explanation content there.
#
# The derived rule — a genre whose `quadrant` is `"explanation"` and
# whose `kinds` is non-empty is a configuration error — is NOT enforced
# here. `docket check` never applies this contract to `docket.ncl` (it
# re-derives the same shape directly in Rust, config.rs, because that is
# also where cross-corpus glob/kind semantics already live); this file
# exists for the documented manual-verification path (`nickel export
# docket.ncl --apply-contract contracts/docket.ncl`). Enforcing the rule
# in two places — a Nickel predicate here and a Rust check there — would
# be exactly the duplication surface this project's own design principles
# forbid, so it has exactly one home: `config::load_config`
# (`ConfigError::ExplanationForbidsKinds`).
let Genre = {
  path     | NonEmptyString,
  kinds    | Array Kind,
  quadrant | Quadrant,
} in

{
  genres | Array Genre,
}
