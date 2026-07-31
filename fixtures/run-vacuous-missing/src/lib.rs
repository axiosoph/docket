// The marker below names a test that was renamed/deleted/typo'd: no
// `missing_test_target` exists in this crate. `cargo test
// missing_test_target -- --exact` still exits 0 (no test failed —
// none ran) and prints `test result: ok. 0 passed; 0 failed; ...`; the
// `printf` here reproduces that exact, measured summary line rather
// than invoking a real `cargo test` (this fixture carries no
// `Cargo.toml` of its own — see fixtures/MANIFEST.md), the same
// stand-in convention `run-pass/`/`run-fail/` already use for `true`/
// `false`.
// docket: missing-test-target :: printf 'running 0 tests\n\ntest result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 1 filtered out; finished in 0.00s\n'

#[test]
fn a_real_test_with_a_different_name() {}
