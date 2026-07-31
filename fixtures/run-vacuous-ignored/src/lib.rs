// The marker below names a real, `#[ignore]`d test. `cargo test
// ignored_test_target -- --exact` *does* collect it (unlike
// `run-vacuous-missing/`'s renamed-away case), then skips it and still
// exits 0, printing `test result: ok. 0 passed; 0 failed; 1 ignored;
// ...`. The `printf` reproduces that exact, measured summary line
// rather than invoking a real `cargo test` — see `run-vacuous-missing/`
// for why.
// docket: ignored-test-target :: printf 'running 1 test\ntest ignored_test_target ... ignored\n\ntest result: ok. 0 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.00s\n'

#[test]
#[ignore]
fn ignored_test_target() {}
