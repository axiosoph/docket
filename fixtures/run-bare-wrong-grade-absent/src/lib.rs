// A bare marker exists for `bare-marker-wrong-grade`, but its claim is
// `evaluator: test` — a grade that needs a command to run, which a bare
// marker cannot offer. It does not count as a match: `docket run
// bare-marker-wrong-grade` reports `absent`, exactly as if this line
// were not here at all (run.rs).
// @docket: bare-marker-wrong-grade
fn ground_values_only() {}
