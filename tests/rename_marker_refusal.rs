//! CLI-level tests for `docket rename`'s refusal to orphan an
//! `@docket:` evaluator marker (MVP.md §4.5).
//!
//! These invoke the built `docket` binary and assert on its actual
//! stdout/stderr bytes and exit code — the layer a real consumer sees —
//! same convention `tests/json_findings.rs` already established for
//! `check --json`: `main.rs` owns the rendering, so a test one layer
//! below it (`rename::plan_rename` alone) cannot see a regression there.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

fn docket_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_docket"))
}

struct Run {
    stdout: String,
    stderr: String,
    status: std::process::ExitStatus,
}

fn run_rename(corpus: &Path, old_id: &str, new_id: &str, write: bool) -> Run {
    let mut cmd = Command::new(docket_bin());
    cmd.arg("rename")
        .arg(old_id)
        .arg(new_id)
        .arg("--corpus")
        .arg(corpus);
    if write {
        cmd.arg("--write");
    }
    let output = cmd.output().expect("docket binary runs");
    Run {
        stdout: String::from_utf8(output.stdout).expect("stdout is utf8"),
        stderr: String::from_utf8(output.stderr).expect("stderr is utf8"),
        status: output.status,
    }
}

/// A throwaway, git-backed corpus directory, cleaned up on drop —
/// mirrors `tests/json_findings.rs`'s own private helper (an
/// integration test cannot reach into the crate's `#[cfg(test)]`
/// modules). Git-backed because `--write` refuses on a dirty tree, and
/// these tests need a real "committed, then unchanged" baseline to
/// confirm nothing was written.
struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!(
            "docket-rename-marker-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn write(&self, relative: &str, contents: &str) {
        let path = self.0.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut f = std::fs::File::create(path).unwrap();
        f.write_all(contents.as_bytes()).unwrap();
    }

    fn read(&self, relative: &str) -> String {
        std::fs::read_to_string(self.0.join(relative)).unwrap()
    }

    fn git_init(&self) {
        for args in [
            vec!["init", "--quiet"],
            vec!["config", "user.email", "test@example.com"],
            vec!["config", "user.name", "Test"],
        ] {
            let status = Command::new("git")
                .arg("-C")
                .arg(&self.0)
                .args(args)
                .status()
                .expect("git must be on PATH to run this test");
            assert!(status.success());
        }
    }

    fn commit_all(&self) {
        for args in [vec!["add", "-A"], vec!["commit", "--quiet", "-m", "init"]] {
            let status = Command::new("git")
                .arg("-C")
                .arg(&self.0)
                .args(args)
                .status()
                .unwrap();
            assert!(status.success());
        }
    }

    /// True iff the working tree carries no uncommitted change — the
    /// same "nothing was written" evidence `refuse_if_dirty` itself
    /// checks against.
    fn is_clean(&self) -> bool {
        let output = Command::new("git")
            .arg("-C")
            .arg(&self.0)
            .args(["status", "--porcelain"])
            .output()
            .unwrap();
        output.stdout.is_empty()
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn basic_docket_ncl() -> &'static str {
    r#"{ genres = [ { path = "docs/**", kinds = ["constraint"], quadrant = "reference" } ] }"#
}

#[test]
fn refuses_a_rename_when_a_marker_names_the_id_and_writes_nothing() {
    let dir = TempDir::new();
    dir.git_init();
    dir.write("docket.ncl", basic_docket_ncl());
    dir.write(
        "docs/a.md",
        "### [old-id]\n\n```claim\nkind: constraint\nevaluator: test\n```\n",
    );
    dir.write("src/lib.rs", "// @docket: old-id :: true\n");
    dir.commit_all();

    let before_doc = dir.read("docs/a.md");
    let before_src = dir.read("src/lib.rs");

    let run = run_rename(dir.path(), "old-id", "new-id", true);
    assert_eq!(
        run.status.code(),
        Some(2),
        "stdout={} stderr={}",
        run.stdout,
        run.stderr
    );
    assert!(
        run.stderr.contains("src/lib.rs:1"),
        "refusal must name the marker's file:line, got: {}",
        run.stderr
    );
    assert!(
        run.stderr.contains("old-id"),
        "refusal must name the id, got: {}",
        run.stderr
    );

    // Nothing written: neither file changed, and the git tree — clean
    // before this run — is still clean after.
    assert_eq!(dir.read("docs/a.md"), before_doc);
    assert_eq!(dir.read("src/lib.rs"), before_src);
    assert!(dir.is_clean(), "a refused rename must leave the tree clean");
}

#[test]
fn a_refusal_also_fires_on_a_dry_run_not_only_with_write() {
    // The marker check is a planning-time refusal (rename.rs's own
    // ordering, "refusals, cheapest first"), not a write-time-only one
    // like the dirty-tree check — a dry-run plan that promised a safe
    // rename would be exactly as misleading as a written one.
    let dir = TempDir::new();
    dir.write("docket.ncl", basic_docket_ncl());
    dir.write(
        "docs/a.md",
        "### [old-id]\n\n```claim\nkind: constraint\nevaluator: test\n```\n",
    );
    dir.write("src/lib.rs", "// @docket: old-id :: true\n");

    let run = run_rename(dir.path(), "old-id", "new-id", false);
    assert_eq!(run.status.code(), Some(2), "stderr={}", run.stderr);
    assert!(run.stderr.contains("src/lib.rs:1"));
}

#[test]
fn a_refusal_names_every_marker_site_not_just_the_first() {
    let dir = TempDir::new();
    dir.git_init();
    dir.write("docket.ncl", basic_docket_ncl());
    dir.write(
        "docs/a.md",
        "### [old-id]\n\n```claim\nkind: constraint\nevaluator: test\n```\n",
    );
    // Two markers for the same claim id, in two different files —
    // MVP.md §4.5's own allowance for "a claim can have more than one
    // marker naming it."
    dir.write("src/one.rs", "// @docket: old-id :: true\n");
    dir.write("src/two.rs", "fn noise() {}\n// @docket: old-id :: false\n");
    dir.commit_all();

    let run = run_rename(dir.path(), "old-id", "new-id", true);
    assert_eq!(run.status.code(), Some(2), "stderr={}", run.stderr);
    assert!(
        run.stderr.contains("src/one.rs:1"),
        "must name the first site, got: {}",
        run.stderr
    );
    assert!(
        run.stderr.contains("src/two.rs:2"),
        "must name the second site too, not just the first, got: {}",
        run.stderr
    );
    assert!(dir.is_clean());
}

#[test]
fn a_claim_with_no_marker_still_renames_normally() {
    // A marker exists in this corpus, but for a *different* claim — the
    // refusal must not over-fire on an unrelated id.
    let dir = TempDir::new();
    dir.git_init();
    dir.write("docket.ncl", basic_docket_ncl());
    dir.write(
        "docs/a.md",
        "### [old-id]\n\n```claim\nkind: constraint\nevaluator: test\n```\n\n### [other-id]\n\n```claim\nkind: constraint\nevaluator: test\n```\n",
    );
    dir.write("src/lib.rs", "// @docket: other-id :: true\n");
    dir.commit_all();

    let run = run_rename(dir.path(), "old-id", "new-id", true);
    assert!(
        run.status.success(),
        "stdout={} stderr={}",
        run.stdout,
        run.stderr
    );
    assert!(dir.read("docs/a.md").contains("### [new-id]"));
    assert!(!dir.read("docs/a.md").contains("old-id"));
}
