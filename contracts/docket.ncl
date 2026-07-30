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

# `kinds = []` is legal and means the genre permits no claim blocks at
# all (MVP.md §2) — the mechanism by which a decision record is prevented
# from carrying normative content.
let Genre = {
  path  | NonEmptyString,
  kinds | Array Kind,
} in

{
  genres | Array Genre,
}
