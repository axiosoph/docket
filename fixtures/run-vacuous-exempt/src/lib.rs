// A stand-in for a proof-kind evaluator (Lean/TLA+/Alloy, none of
// which the runner has an output recognizer for) whose command
// happens to print text that *would* match the cargo-shaped vacuity
// signal if the detector ran — deliberately, to prove the `!` exempts
// it rather than merely being untested against a lucky case. The
// author's marker asserts, once, that the exit status alone is
// conclusive for this evaluator.
// @docket: exempt-target! :: printf 'test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n'
