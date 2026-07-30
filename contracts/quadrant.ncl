# Canonical home for the Quadrant enum (MVP.md §2), imported rather than
# restated wherever a `quadrant` value needs validating — the same pattern
# claim.ncl's `Kind` follows for docket.ncl's `kinds` field.
#
# Divio's four documentation quadrants: which question a genre's documents
# answer. Closed (only these four) so a corpus's genre taxonomy is
# comparable to another corpus's, not an open vocabulary each project
# reinvents.
{
  Quadrant = std.contract.from_predicate (fun v =>
    v == 'tutorial    || v == "tutorial"
    || v == 'how-to    || v == "how-to"
    || v == 'reference || v == "reference"
    || v == 'explanation || v == "explanation"
  ),
}
