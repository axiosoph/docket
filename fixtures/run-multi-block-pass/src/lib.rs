// The marker below reproduces the real shape a `cargo test` invocation
// prints on a crate that has both `#[cfg(test)] mod tests` and
// doc-comment examples: two binaries, two `test result: ` summary
// blocks in one invocation. The unittest block genuinely ran three
// tests; the doctest block — filtered to nothing by a marker author's
// exact-name filter, same as `run-vacuous-missing/` — reports the
// identical `0 passed; 0 failed` shape a renamed/deleted/`#[ignore]`d
// test does. The claim must still discharge: a block with real activity
// means the whole invocation is not vacuous, regardless of what a
// sibling block reports. The `printf` reproduces that exact, measured
// two-block output rather than invoking a real `cargo test` — see
// `run-vacuous-missing/` for why (this fixture carries no `Cargo.toml`
// of its own, fixtures/MANIFEST.md).
// @docket: multi-block-target :: printf 'running 3 tests\ntest tests::a ... ok\ntest tests::b ... ok\ntest tests::c ... ok\n\ntest result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n\nrunning 0 tests\n\ntest result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 1 filtered out; finished in 0.00s\n'

#[test]
fn a() {}
#[test]
fn b() {}
#[test]
fn c() {}
