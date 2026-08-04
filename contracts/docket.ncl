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
let Genre = {
  path     | NonEmptyString,
  kinds    | Array Kind,
  quadrant | Quadrant,
} in

# The derived rule: a genre whose `quadrant` is `"explanation"` and whose
# `kinds` is non-empty is a configuration error — explanation carries
# rationale, not checkable claims. Named per-offender so the diagnostic
# says which genre entry is wrong, matching what a hand-written check
# would report. `docket check` applies this contract directly
# (`config::load_config`) — this is now the single, sole home for the
# config's shape and its derived rules; Rust converts the validated JSON
# into typed values but does not re-check any of it.
let GenresValid = std.contract.from_validator (fun genres =>
  let offenders = std.array.filter
    (fun g => g.quadrant == "explanation" && std.array.length g.kinds > 0)
    genres
  in
  if std.array.length offenders == 0 then
    'Ok
  else
    let g = std.array.first offenders in
    'Error {
      message = "genre \"%{g.path}\" has quadrant \"explanation\" but permits kinds [%{std.string.join ", " g.kinds}] — explanation carries rationale, not checkable claims",
    }
) in

{
  genres | Array Genre | GenresValid,
}
