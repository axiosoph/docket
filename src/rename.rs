//! `docket rename <old-id> <new-id>`: rename a claim id and every
//! reference to it — the definition (any of the three forms, MVP.md
//! §1.1), every bare `depends`/`because` entry naming it, and every prose
//! link naming it — across the corpus.
//!
//! The design here follows the head's own ruling rather than improvising:
//!
//! 1. **Closed by construction.** [`extract::find_rename_sites`] is the
//!    only source of byte ranges, and [`apply_edits`] is the only thing
//!    that writes into them — it can express nothing but "replace this
//!    verified `old_id`-shaped span with `new_id`'s own bytes." There is
//!    no path from here to touching claim-block bytes, prose, or anything
//!    else.
//! 2. **Staged, never incremental.** [`plan_rename`] computes every edit,
//!    applies them to in-memory copies, re-runs the register over the
//!    in-memory corpus ([`corpus::load_corpus_with_overrides`]), and
//!    compares against the original — see [`Snapshot`]. Disk is never
//!    touched until every one of those steps has already succeeded.
//! 3. **The invariant is not the index.** [`Snapshot`] carries the
//!    register's `{index, diagnostics}` output AND every claim's
//!    `prose_links`/`prose_code` — the index alone misses a `line` that
//!    moved for the wrong reason, a diagnostic the index never carried,
//!    and prose the anchor form's scope depends on.
//! 4. **Write, never commit.** [`write_changes`] only ever calls
//!    `std::fs::write`; nothing here touches git.
//! 5. **`--write` is opt-in.** Enforced by `main.rs`, not here: this
//!    module always computes and verifies a plan; whether it is then
//!    applied to disk is the caller's decision.
//! 6. **Refuse on a dirty working tree.** [`refuse_if_dirty`], required by
//!    `main.rs` before it ever calls [`write_changes`].
//! 7. **Never allocate or normalize an id.** Every id string this module
//!    handles is either the caller's own `old_id`/`new_id` argument or a
//!    byte-for-byte substring the extractor located — nothing here ever
//!    invents or reshapes one.
//! 8. **Refuse rather than orphan an evaluator marker.** An `@docket:`
//!    marker (marker.rs) is not one of the reference kinds §1.2/§1.3
//!    define — it lives outside the corpus and is resolved by a wholly
//!    separate scan — so it is out of reach of every primitive above.
//!    [`marker_sites_naming`] finds every marker naming `old_id`, and
//!    `old_id` resolving to a claim with at least one is refused before
//!    any edit is computed, per the head's ruling that a corpus-side
//!    rename must never leave `docket run <new_id>` reporting `absent`
//!    while `docket check` stays green.

use crate::checks::{self, Diagnostic, RegisterError, RegisterResult};
use crate::config::Config;
use crate::corpus::{self, CorpusError, LoadedCorpus};
use crate::extract;
use crate::marker::Marker;
use crate::model::{self, Index, IndexClaim};
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum RenameError {
    #[error("no claim with id {0:?} in this corpus")]
    UnknownSourceId(String),
    #[error(
        "{old_id:?} is named by {count} `@docket:` evaluator marker(s) — rename refuses to orphan them (a marker lives outside the corpus `check` resolves, so `docket check` would still exit 0 after the break):\n{sites}\nUpdate each site's `@docket: {old_id}` to `@docket: {new_id}` by hand, then rename again."
    )]
    MarkerSitesRemain {
        old_id: String,
        new_id: String,
        count: usize,
        sites: String,
    },
    #[error(
        "{new_id:?} is not a well-formed claim id (lowercase kebab-case, e.g. \"lock-groundness\")"
    )]
    MalformedTargetId { new_id: String },
    #[error(
        "{new_id:?} already names a definition at {file}:{line} — rename never overwrites an existing id"
    )]
    TargetAlreadyExists {
        new_id: String,
        file: String,
        line: usize,
    },
    #[error(
        "working tree is dirty under {corpus_root} — commit or stash before `docket rename --write` (this is what makes `git checkout .` a complete undo)"
    )]
    DirtyWorkingTree { corpus_root: String },
    #[error("could not check working tree state under {corpus_root}: {detail}")]
    GitStatus { corpus_root: String, detail: String },
    #[error(
        "rename invariant diverged after applying every computed edit — nothing was written.\n{0}"
    )]
    InvariantDiverged(String),
    #[error("could not read {path}: {source}")]
    Read {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("could not write {path}: {source}")]
    Write {
        path: String,
        #[source]
        source: std::io::Error,
    },
    /// Should-never-happen defects in the site-finder or the edit
    /// applicator itself — surfaced rather than silently producing a
    /// corrupted file, per the same "no wasted tokens, no silent
    /// failures" floor every other module here holds to.
    #[error("internal error computing rename edits: {0}")]
    Internal(String),
    #[error(transparent)]
    Corpus(#[from] CorpusError),
    #[error(transparent)]
    Register(#[from] RegisterError),
}

/// One touched file's before/after content, plus how many sites in it
/// were rewritten — the `docket rename` plan's per-file line.
#[derive(Debug, Clone)]
pub struct FileChange {
    pub old_content: String,
    pub new_content: String,
    pub edit_count: usize,
}

/// The full result of a verified — but not yet written — rename.
/// `changes` is empty only if `old_id` were somehow found with no sites at
/// all, which [`plan_rename`] treats as impossible for a real claim (its
/// own definition is always at least one site) rather than a legitimate
/// empty plan.
#[derive(Debug, Clone)]
pub struct RenamePlan {
    pub old_id: String,
    pub new_id: String,
    pub changes: BTreeMap<String, FileChange>,
}

impl RenamePlan {
    pub fn total_edits(&self) -> usize {
        self.changes.values().map(|c| c.edit_count).sum()
    }
}

/// Every `@docket:` marker (marker.rs) naming `old_id`, as `file:line` —
/// `scan_markers` already returns markers sorted by `(file, line)`, so
/// this preserves that order rather than re-sorting. Named exhaustively,
/// not just the first: a refusal that under-reports sites would be the
/// same silent-partiality defect the refusal exists to prevent.
fn marker_sites_naming(markers: &[Marker], old_id: &str) -> Vec<String> {
    markers
        .iter()
        .filter(|m| m.id == old_id)
        .map(|m| format!("{}:{}", m.file, m.line))
        .collect()
}

/// Apply every `sites` edit to `source`, replacing each with `new_id`'s
/// own bytes. The writer's whole primitive: verified, non-overlapping
/// `[s,e)` spans in, `new_id` substituted at each, nothing else touched.
/// Each site's span is re-verified against `old_id` here — a second,
/// independent check beyond whatever `find_rename_sites` already did —
/// because this is the last point before the bytes are actually rewritten.
fn apply_edits(
    source: &str,
    sites: &[extract::RenameSite],
    old_id: &str,
    new_id: &str,
) -> Result<String, RenameError> {
    let mut sorted = sites.to_vec();
    sorted.sort_by_key(|s| s.start);
    for w in sorted.windows(2) {
        if w[0].end > w[1].start {
            return Err(RenameError::Internal(format!(
                "overlapping rename sites at bytes {}..{} and {}..{}",
                w[0].start, w[0].end, w[1].start, w[1].end
            )));
        }
    }

    let mut out = String::with_capacity(source.len());
    let mut cursor = 0usize;
    for site in &sorted {
        let Some(found) = source.get(site.start..site.end) else {
            return Err(RenameError::Internal(format!(
                "rename site {}..{} is not a valid byte range in this document",
                site.start, site.end
            )));
        };
        if found != old_id {
            return Err(RenameError::Internal(format!(
                "rename site {}..{} contains {found:?}, not {old_id:?} — refusing to guess",
                site.start, site.end
            )));
        }
        out.push_str(&source[cursor..site.start]);
        out.push_str(new_id);
        cursor = site.end;
    }
    out.push_str(&source[cursor..]);
    Ok(out)
}

// --- the staged invariant -------------------------------------------------

/// The register's full `{index, diagnostics}` output, plus every claim's
/// `prose_links`/`prose_code` — the rename dispatch's own statement of
/// what must be identical before and after, modulo the id substitution.
/// The index alone is insufficient (three separate leaks, per the
/// dispatch): `line` lives in the index but a diagnostic's own `line`
/// does not come from it, `orphaned-because`/`dangling-reference` and
/// every other `Warn`-severity finding are diagnostics with no index
/// entry at all, and a claim's prose scope (which the anchor form
/// depends on) is nowhere in the index either.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Snapshot {
    index: Index,
    diagnostics: Vec<Diagnostic>,
    /// claim id -> (prose_links, prose_code), sorted by id via `BTreeMap`
    /// so two snapshots compare independent of extraction order.
    claim_prose: BTreeMap<String, (Vec<String>, Vec<String>)>,
}

fn snapshot(loaded: &LoadedCorpus, result: &RegisterResult) -> Snapshot {
    let mut diagnostics = result.report.diagnostics.clone();
    // Diagnostics carry no natural corpus-wide order beyond "however
    // register.ncl emitted them" — sort so two snapshots compare by
    // content, not by incidental evaluation order.
    diagnostics.sort_by(|a, b| {
        (&a.check, &a.file, a.line, &a.message).cmp(&(&b.check, &b.file, b.line, &b.message))
    });
    let claim_prose = loaded
        .corpus
        .claims
        .iter()
        .map(|c| (c.id.clone(), (c.prose_links.clone(), c.prose_code.clone())))
        .collect();
    Snapshot {
        index: result.index.clone(),
        diagnostics,
        claim_prose,
    }
}

fn substitute_id(s: &str, old_id: &str, new_id: &str) -> String {
    if s == old_id {
        new_id.to_string()
    } else {
        s.to_string()
    }
}

/// A prose-link entry renamed the same way [`extract::find_rename_sites`]'s
/// `ProseLink` sites are: the whole entry if it bare-names `old_id`, or
/// just the fragment after the last `#` if that names `old_id` (a doc
/// anchor whose anchor happens to equal `old_id` textually is a different
/// citation — a section, not this claim — and is left alone, matching
/// `find_rename_sites`' own scoping).
fn substitute_link(link: &str, old_id: &str, new_id: &str) -> String {
    if link == old_id {
        return new_id.to_string();
    }
    if let Some((prefix, frag)) = link.rsplit_once('#')
        && frag == old_id
    {
        return format!("{prefix}#{new_id}");
    }
    link.to_string()
}

/// Rewrite `snap` as if `old_id` had always been `new_id` — the "modulo
/// the old→new id substitution" the invariant allows. Applied to the
/// BEFORE snapshot; the result must equal the real AFTER snapshot exactly
/// for a rename to be considered safe. A diagnostic's `message` gets a
/// literal substring substitution (a register message can embed an id
/// inside otherwise-unstructured text, e.g. "duplicate id: old-id"); every
/// other field is a structured value compared by exact value, not text.
fn substitute_snapshot(snap: &Snapshot, old_id: &str, new_id: &str) -> Snapshot {
    let claims = snap
        .index
        .claims
        .iter()
        .map(|(id, c)| {
            let new_key = if id == old_id {
                new_id.to_string()
            } else {
                id.clone()
            };
            let new_claim = IndexClaim {
                file: c.file.clone(),
                line: c.line,
                kind: c.kind.clone(),
                evaluator: c.evaluator.clone(),
                depends: c
                    .depends
                    .iter()
                    .map(|d| substitute_id(d, old_id, new_id))
                    .collect(),
                because: c
                    .because
                    .iter()
                    .map(|d| substitute_id(d, old_id, new_id))
                    .collect(),
            };
            (new_key, new_claim)
        })
        .collect();

    let diagnostics = snap
        .diagnostics
        .iter()
        .map(|d| Diagnostic {
            check: d.check.clone(),
            severity: d.severity,
            file: d.file.clone(),
            line: d.line,
            claim_id: d
                .claim_id
                .as_deref()
                .map(|id| substitute_id(id, old_id, new_id)),
            message: d.message.replace(old_id, new_id),
        })
        .collect();

    let claim_prose = snap
        .claim_prose
        .iter()
        .map(|(id, (links, code))| {
            let new_key = if id == old_id {
                new_id.to_string()
            } else {
                id.clone()
            };
            let new_links = links
                .iter()
                .map(|l| substitute_link(l, old_id, new_id))
                .collect();
            // `prose_code` entries are literal code-span text, never a
            // reference — the id substitution never touches them.
            (new_key, (new_links, code.clone()))
        })
        .collect();

    Snapshot {
        index: Index {
            claims,
            documents: snap.index.documents.clone(),
        },
        diagnostics,
        claim_prose,
    }
}

/// A short, readable account of exactly where `expected` (the substituted
/// BEFORE snapshot) and `actual` (the real AFTER snapshot) disagree — the
/// failure message IS the feature (README.md/MVP.md's own standard for
/// every refusal here): a bare "it didn't work" would defeat the point of
/// staging the comparison at all.
fn diff_snapshots(expected: &Snapshot, actual: &Snapshot) -> String {
    let mut lines = Vec::new();

    let expected_ids: std::collections::BTreeSet<_> = expected.index.claims.keys().collect();
    let actual_ids: std::collections::BTreeSet<_> = actual.index.claims.keys().collect();
    for missing in expected_ids.difference(&actual_ids) {
        lines.push(format!(
            "  index: claim {missing:?} expected but missing after rename"
        ));
    }
    for extra in actual_ids.difference(&expected_ids) {
        lines.push(format!(
            "  index: claim {extra:?} present after rename but not expected"
        ));
    }
    for id in expected_ids.intersection(&actual_ids) {
        let e = &expected.index.claims[*id];
        let a = &actual.index.claims[*id];
        if e != a {
            lines.push(format!(
                "  index: claim {id:?} diverged:\n    expected {e:?}\n    actual   {a:?}"
            ));
        }
    }

    if expected.diagnostics != actual.diagnostics {
        lines.push(format!(
            "  diagnostics diverged:\n    expected {:#?}\n    actual   {:#?}",
            expected.diagnostics, actual.diagnostics
        ));
    }

    let expected_claims: std::collections::BTreeSet<_> = expected.claim_prose.keys().collect();
    let actual_claims: std::collections::BTreeSet<_> = actual.claim_prose.keys().collect();
    for id in expected_claims.intersection(&actual_claims) {
        let e = &expected.claim_prose[*id];
        let a = &actual.claim_prose[*id];
        if e != a {
            lines.push(format!(
                "  claim {id:?} prose diverged:\n    expected links={:?} code={:?}\n    actual   links={:?} code={:?}",
                e.0, e.1, a.0, a.1
            ));
        }
    }
    for missing in expected_claims.difference(&actual_claims) {
        lines.push(format!(
            "  claim {missing:?} prose expected but missing after rename"
        ));
    }
    for extra in actual_claims.difference(&expected_claims) {
        lines.push(format!(
            "  claim {extra:?} prose present after rename but not expected"
        ));
    }

    lines.join("\n")
}

// --- the plan --------------------------------------------------------------

/// Compute, apply in-memory, and verify a rename — never touching disk.
/// `loaded`/`config`/`register_path`/`markers` are the corpus as already
/// loaded by the caller (the same shape every other `main.rs` command
/// loads once); this function loads a SECOND, in-memory-overridden copy
/// internally to compute the AFTER snapshot (step 2 of the head's
/// ruling), and returns only once that snapshot has been verified against
/// the BEFORE one.
pub fn plan_rename(
    corpus_root: &Path,
    loaded: &LoadedCorpus,
    config: &Config,
    register_path: &Path,
    markers: &[Marker],
    old_id: &str,
    new_id: &str,
) -> Result<RenamePlan, RenameError> {
    // --- refusals, cheapest first --------------------------------------

    if loaded.corpus.claims.iter().all(|c| c.id != old_id) {
        return Err(RenameError::UnknownSourceId(old_id.to_string()));
    }

    // A marker is not one of the reference kinds §1.2/§1.3 define — it
    // lives outside the corpus, resolved by a wholly separate scan
    // (marker.rs), and `apply_edits`' only primitive is a byte-exact
    // rewrite of a corpus-recognized reference span. So this cannot be
    // fixed by teaching rename to also edit markers (MVP.md §4.5's own
    // ruling on the point): a renamed claim whose marker still names
    // `old_id` would evidence-drop silently — `docket check` sees no
    // citation graph over markers at all, so it stays green while
    // `docket run <new_id>` reports `absent`. Refuse instead, and name
    // every site so the fix is two steps, not a search.
    let marker_sites = marker_sites_naming(markers, old_id);
    if !marker_sites.is_empty() {
        return Err(RenameError::MarkerSitesRemain {
            old_id: old_id.to_string(),
            new_id: new_id.to_string(),
            count: marker_sites.len(),
            sites: marker_sites
                .iter()
                .map(|s| format!("  {s}"))
                .collect::<Vec<_>>()
                .join("\n"),
        });
    }

    if !model::is_valid_claim_id(new_id) {
        return Err(RenameError::MalformedTargetId {
            new_id: new_id.to_string(),
        });
    }

    if let Some(existing) = loaded.corpus.claims.iter().find(|c| c.id == new_id) {
        return Err(RenameError::TargetAlreadyExists {
            new_id: new_id.to_string(),
            file: existing.file.clone(),
            line: existing.heading_line.0,
        });
    }
    if let Some(existing) = loaded
        .unregistered_definitions
        .iter()
        .find(|d| d.id == new_id)
    {
        return Err(RenameError::TargetAlreadyExists {
            new_id: new_id.to_string(),
            file: existing.file.clone(),
            line: existing.line.0,
        });
    }

    // --- compute every edit, per file -----------------------------------

    let mut changes: BTreeMap<String, FileChange> = BTreeMap::new();
    for doc in &loaded.corpus.documents {
        let path = corpus_root.join(&doc.file);
        let old_content = std::fs::read_to_string(&path).map_err(|source| RenameError::Read {
            path: doc.file.clone(),
            source,
        })?;
        let sites = extract::find_rename_sites(&old_content, old_id);
        if sites.is_empty() {
            continue;
        }
        let new_content = apply_edits(&old_content, &sites, old_id, new_id)?;
        changes.insert(
            doc.file.clone(),
            FileChange {
                edit_count: sites.len(),
                old_content,
                new_content,
            },
        );
    }

    // A real claim always has a definition, so this cannot legitimately
    // be empty — the "rename touching zero references is suspicious"
    // ruling's own reasoning, restated as a defensive invariant rather
    // than trusted to the caller's `UnknownSourceId` check alone.
    if changes.is_empty() {
        return Err(RenameError::Internal(format!(
            "{old_id:?} resolved to a claim but no rename site was found anywhere in the corpus"
        )));
    }

    // --- stage: apply to in-memory copies, re-run the register ----------

    let before_result = checks::run_checks(loaded, config, register_path, markers)?;
    let before_snapshot = snapshot(loaded, &before_result);
    let expected = substitute_snapshot(&before_snapshot, old_id, new_id);

    let overrides: BTreeMap<String, String> = changes
        .iter()
        .map(|(file, change)| (file.clone(), change.new_content.clone()))
        .collect();
    let after_loaded = corpus::load_corpus_with_overrides(corpus_root, config, &overrides)?;
    let after_result = checks::run_checks(&after_loaded, config, register_path, markers)?;
    let actual = snapshot(&after_loaded, &after_result);

    if expected != actual {
        return Err(RenameError::InvariantDiverged(diff_snapshots(
            &expected, &actual,
        )));
    }

    Ok(RenamePlan {
        old_id: old_id.to_string(),
        new_id: new_id.to_string(),
        changes,
    })
}

/// Write every touched file's new content to disk — the only place in
/// this module (or, by the head's ruling, in `docket` at all) that
/// mutates the corpus. Never called except by `main.rs`'s `--write` path,
/// and never on a plan that has not already passed [`plan_rename`]'s
/// invariant check.
pub fn write_changes(corpus_root: &Path, plan: &RenamePlan) -> Result<(), RenameError> {
    for (file, change) in &plan.changes {
        let path = corpus_root.join(file);
        std::fs::write(&path, &change.new_content).map_err(|source| RenameError::Write {
            path: file.clone(),
            source,
        })?;
    }
    Ok(())
}

/// Refuse if `corpus_root`'s working tree carries any uncommitted change
/// under it — what makes `git checkout .` (run from `corpus_root`) a
/// complete undo of a `--write`. Only ever called by `main.rs` on the
/// `--write` path; a dry-run plan makes no changes, so a dirty tree poses
/// it no risk.
pub fn refuse_if_dirty(corpus_root: &Path) -> Result<(), RenameError> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(corpus_root)
        .args(["status", "--porcelain", "--", "."])
        .output()
        .map_err(|e| RenameError::GitStatus {
            corpus_root: corpus_root.display().to_string(),
            detail: e.to_string(),
        })?;
    if !output.status.success() {
        return Err(RenameError::GitStatus {
            corpus_root: corpus_root.display().to_string(),
            detail: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }
    if output.stdout.is_empty() {
        Ok(())
    } else {
        Err(RenameError::DirtyWorkingTree {
            corpus_root: corpus_root.display().to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::load_config;
    use crate::contracts::MaterializedContracts;
    use crate::corpus::load_corpus;
    use crate::extract::RenameSiteKind;
    use crate::marker::scan_markers;
    use std::io::Write;

    struct TempDir(std::path::PathBuf);
    impl TempDir {
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
            let status = std::process::Command::new("git")
                .arg("-C")
                .arg(&self.0)
                .args(["init", "--quiet"])
                .status()
                .expect("git must be on PATH to run this test");
            assert!(status.success());
            // A user identity is required for `git commit`, which the
            // dirty-tree tests below use to get a clean starting state.
            for (key, value) in [("user.email", "test@example.com"), ("user.name", "Test")] {
                std::process::Command::new("git")
                    .arg("-C")
                    .arg(&self.0)
                    .args(["config", key, value])
                    .status()
                    .unwrap();
            }
        }
        fn commit_all(&self) {
            std::process::Command::new("git")
                .arg("-C")
                .arg(&self.0)
                .args(["add", "-A"])
                .status()
                .unwrap();
            std::process::Command::new("git")
                .arg("-C")
                .arg(&self.0)
                .args(["commit", "--quiet", "-m", "init"])
                .status()
                .unwrap();
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn tempdir() -> TempDir {
        let dir = std::env::temp_dir().join(format!(
            "docket-rename-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }

    /// Everything a `plan_rename` call needs, loaded once — mirrors
    /// `main.rs`'s own `run_check` orchestration (config, corpus,
    /// markers, the embedded register) so these tests exercise the real
    /// wiring, not a stand-in for it.
    struct Harness {
        _contracts: MaterializedContracts,
        loaded: LoadedCorpus,
        config: Config,
        markers: Vec<Marker>,
        register_path: std::path::PathBuf,
    }
    fn harness(dir: &TempDir) -> Harness {
        let config = load_config(dir.path()).expect("valid docket.ncl");
        let loaded = load_corpus(dir.path(), &config).expect("corpus loads");
        let markers = scan_markers(dir.path()).expect("marker scan");
        let contracts = MaterializedContracts::new().expect("contracts materialize");
        let register_path = contracts.register_path();
        Harness {
            _contracts: contracts,
            loaded,
            config,
            markers,
            register_path,
        }
    }

    fn basic_docket_ncl() -> &'static str {
        r#"{ genres = [ { path = "docs/**", kinds = ["constraint"], quadrant = "reference" } ] }"#
    }

    // --- the three anchor forms, end to end ------------------------------

    #[test]
    fn renames_a_heading_form_definition_and_writes_it() {
        let dir = tempdir();
        dir.write("docket.ncl", basic_docket_ncl());
        dir.write(
            "docs/a.md",
            "### [old-id]\n\nEvery lock value MUST be ground.\n\n```claim\nkind: constraint\nevaluator: test\n```\n",
        );
        let h = harness(&dir);
        let plan = plan_rename(
            dir.path(),
            &h.loaded,
            &h.config,
            &h.register_path,
            &h.markers,
            "old-id",
            "new-id",
        )
        .expect("rename plan succeeds");
        assert_eq!(plan.changes.len(), 1);
        assert_eq!(plan.total_edits(), 1);
        write_changes(dir.path(), &plan).expect("write succeeds");
        assert_eq!(
            dir.read("docs/a.md"),
            "### [new-id]\n\nEvery lock value MUST be ground.\n\n```claim\nkind: constraint\nevaluator: test\n```\n"
        );
    }

    #[test]
    fn renames_a_bold_form_definition() {
        let dir = tempdir();
        dir.write("docket.ncl", basic_docket_ncl());
        dir.write(
            "docs/a.md",
            "**[old-id]**: Every lock value MUST be ground.\n\n```claim\nkind: constraint\nevaluator: test\n```\n",
        );
        let h = harness(&dir);
        let plan = plan_rename(
            dir.path(),
            &h.loaded,
            &h.config,
            &h.register_path,
            &h.markers,
            "old-id",
            "new-id",
        )
        .expect("rename plan succeeds");
        write_changes(dir.path(), &plan).unwrap();
        assert!(dir.read("docs/a.md").starts_with("**[new-id]**:"));
    }

    #[test]
    fn renames_an_html_form_definition_whose_adjacency_classification_survives() {
        // The dispatch's own required case: an `<a id>` anchor immediately
        // beside a heading (a heading-adjacent, reclassified anchor,
        // MVP.md §1.1) must still resolve to exactly one rename site and
        // must not disturb the heading it sits beside.
        let dir = tempdir();
        dir.write("docket.ncl", basic_docket_ncl());
        dir.write(
            "docs/a.md",
            "<a id=\"old-id\"></a>\n## A section title\n\nEvery lock value MUST be ground.\n\n```claim\nkind: constraint\nevaluator: test\n```\n",
        );
        let h = harness(&dir);
        let plan = plan_rename(
            dir.path(),
            &h.loaded,
            &h.config,
            &h.register_path,
            &h.markers,
            "old-id",
            "new-id",
        )
        .expect("rename plan succeeds");
        assert_eq!(plan.total_edits(), 1);
        write_changes(dir.path(), &plan).unwrap();
        let after = dir.read("docs/a.md");
        assert!(after.starts_with("<a id=\"new-id\"></a>\n## A section title\n"));
    }

    // --- depends, because, prose link, and the anchor together ----------

    #[test]
    fn renames_every_reference_kind_and_nothing_else() {
        let dir = tempdir();
        dir.write("docket.ncl", basic_docket_ncl());
        dir.write(
            "docs/a.md",
            "### [old-id]\n\nSome prose. See [it](old-id) and [via anchor](docs/b.md#old-id).\n\n```claim\nkind: constraint\nevaluator: test\n```\n",
        );
        dir.write(
            "docs/b.md",
            "### [b]\n\n```claim\nkind: constraint\nevaluator: test\ndepends: [old-id]\nbecause: [old-id]\n```\n",
        );
        let h = harness(&dir);
        let plan = plan_rename(
            dir.path(),
            &h.loaded,
            &h.config,
            &h.register_path,
            &h.markers,
            "old-id",
            "new-id",
        )
        .expect("rename plan succeeds");
        assert_eq!(plan.changes.len(), 2);
        write_changes(dir.path(), &plan).unwrap();

        let a = dir.read("docs/a.md");
        assert!(a.contains("### [new-id]"));
        assert!(a.contains("[it](new-id)"));
        assert!(a.contains("[via anchor](docs/b.md#new-id)"));
        assert!(!a.contains("old-id"));

        let b = dir.read("docs/b.md");
        assert!(b.contains("depends: [new-id]"));
        assert!(b.contains("because: [new-id]"));
        assert!(!b.contains("old-id"));
    }

    #[test]
    fn a_diagnostics_own_claim_id_is_substituted_too() {
        // `Diagnostic` grew a `claim_id` field after this module's
        // `Snapshot`/`substitute_snapshot` pair was written (checks.rs);
        // an unresolved `because` entry produces an `orphaned-because`
        // warning whose `claim_id` is the *citing* claim's own id
        // (register.ncl's `refs_check`) — here, `old-id` itself. If
        // `substitute_snapshot` left that field unrewritten, the
        // substituted BEFORE snapshot would keep `claim_id: "old-id"`
        // while the real AFTER snapshot reports `"new-id"`, and a rename
        // that is otherwise perfectly safe would be refused as a false
        // InvariantDiverged.
        let dir = tempdir();
        dir.write("docket.ncl", basic_docket_ncl());
        dir.write(
            "docs/a.md",
            "### [old-id]\n\n```claim\nkind: constraint\nevaluator: test\nbecause: [does-not-exist]\n```\n",
        );
        let h = harness(&dir);
        let plan = plan_rename(
            dir.path(),
            &h.loaded,
            &h.config,
            &h.register_path,
            &h.markers,
            "old-id",
            "new-id",
        )
        .expect(
            "rename plan succeeds; the orphaned-because warning must not look like a divergence",
        );
        write_changes(dir.path(), &plan).unwrap();
        assert!(dir.read("docs/a.md").contains("### [new-id]"));
    }

    // --- refusals ----------------------------------------------------------

    #[test]
    fn refuses_an_unknown_source_id() {
        let dir = tempdir();
        dir.write("docket.ncl", basic_docket_ncl());
        dir.write(
            "docs/a.md",
            "### [real-id]\n\n```claim\nkind: constraint\nevaluator: test\n```\n",
        );
        let h = harness(&dir);
        let err = plan_rename(
            dir.path(),
            &h.loaded,
            &h.config,
            &h.register_path,
            &h.markers,
            "typo-id",
            "new-id",
        )
        .unwrap_err();
        assert!(matches!(err, RenameError::UnknownSourceId(id) if id == "typo-id"));
    }

    #[test]
    fn refuses_a_malformed_target_id() {
        let dir = tempdir();
        dir.write("docket.ncl", basic_docket_ncl());
        dir.write(
            "docs/a.md",
            "### [old-id]\n\n```claim\nkind: constraint\nevaluator: test\n```\n",
        );
        let h = harness(&dir);
        let err = plan_rename(
            dir.path(),
            &h.loaded,
            &h.config,
            &h.register_path,
            &h.markers,
            "old-id",
            "Not_Kebab",
        )
        .unwrap_err();
        assert!(matches!(err, RenameError::MalformedTargetId { .. }));
    }

    #[test]
    fn refuses_a_target_id_that_already_exists() {
        let dir = tempdir();
        dir.write("docket.ncl", basic_docket_ncl());
        dir.write(
            "docs/a.md",
            "### [old-id]\n\n```claim\nkind: constraint\nevaluator: test\n```\n\n### [taken-id]\n\n```claim\nkind: constraint\nevaluator: test\n```\n",
        );
        let h = harness(&dir);
        let err = plan_rename(
            dir.path(),
            &h.loaded,
            &h.config,
            &h.register_path,
            &h.markers,
            "old-id",
            "taken-id",
        )
        .unwrap_err();
        match err {
            RenameError::TargetAlreadyExists { new_id, file, line } => {
                assert_eq!(new_id, "taken-id");
                assert_eq!(file, "docs/a.md");
                assert!(line > 0);
            }
            other => panic!("expected TargetAlreadyExists, got {other:?}"),
        }
    }

    #[test]
    fn refuses_a_target_id_that_exists_only_as_an_unregistered_definition() {
        // "Must not already exist" covers any recognized definition, not
        // only a claimed one — an unregistered `[taken-id]` heading with
        // no claim block still names something.
        let dir = tempdir();
        dir.write("docket.ncl", basic_docket_ncl());
        dir.write(
            "docs/a.md",
            "### [old-id]\n\n```claim\nkind: constraint\nevaluator: test\n```\n\n### [taken-id]\n\nNo claim block follows this one.\n",
        );
        let h = harness(&dir);
        let err = plan_rename(
            dir.path(),
            &h.loaded,
            &h.config,
            &h.register_path,
            &h.markers,
            "old-id",
            "taken-id",
        )
        .unwrap_err();
        assert!(matches!(err, RenameError::TargetAlreadyExists { .. }));
    }

    #[test]
    fn refuses_a_dirty_working_tree_before_write() {
        let dir = tempdir();
        dir.git_init();
        dir.write("docket.ncl", basic_docket_ncl());
        dir.write(
            "docs/a.md",
            "### [old-id]\n\n```claim\nkind: constraint\nevaluator: test\n```\n",
        );
        dir.commit_all();
        // Dirty it: an untracked file is enough to fail `git status
        // --porcelain`'s emptiness check.
        dir.write("docs/scratch.md", "uncommitted\n");

        let err = refuse_if_dirty(dir.path()).unwrap_err();
        assert!(matches!(err, RenameError::DirtyWorkingTree { .. }));
    }

    #[test]
    fn a_clean_working_tree_is_not_refused() {
        let dir = tempdir();
        dir.git_init();
        dir.write("docket.ncl", basic_docket_ncl());
        dir.write(
            "docs/a.md",
            "### [old-id]\n\n```claim\nkind: constraint\nevaluator: test\n```\n",
        );
        dir.commit_all();

        refuse_if_dirty(dir.path()).expect("a fully committed tree must not be refused");
    }

    #[test]
    fn dry_run_never_touches_disk() {
        let dir = tempdir();
        dir.write("docket.ncl", basic_docket_ncl());
        dir.write(
            "docs/a.md",
            "### [old-id]\n\n```claim\nkind: constraint\nevaluator: test\n```\n",
        );
        let original = dir.read("docs/a.md");
        let h = harness(&dir);
        let plan = plan_rename(
            dir.path(),
            &h.loaded,
            &h.config,
            &h.register_path,
            &h.markers,
            "old-id",
            "new-id",
        )
        .expect("rename plan succeeds");
        assert_eq!(plan.total_edits(), 1);
        // `plan_rename` alone — no `write_changes` call — must leave the
        // file exactly as it was.
        assert_eq!(dir.read("docs/a.md"), original);
    }

    // --- the invariant catches a defect and writes nothing ---------------

    #[test]
    fn an_honestly_reachable_invariant_failure_writes_nothing() {
        // A real, documented residual — not a fabricated scenario the
        // mechanism cannot reach: `find_yaml_ref_spans` only recognizes a
        // `depends`/`because` flow sequence written on ONE line
        // (`extract.rs`'s own doc comment states this scope explicitly).
        // The register (via `register.ncl`, real YAML parsing) has no
        // such limit — a `depends` array split across lines resolves
        // identically either way. So a claim whose `depends` entry is
        // legitimately multi-line YAML is a citation the writer's site
        // finder will genuinely miss, while the register still sees it:
        // exactly the shape the staged invariant exists to catch. This
        // reaches the divergence through `plan_rename` itself, not a
        // hand-built snapshot.
        let dir = tempdir();
        dir.write("docket.ncl", basic_docket_ncl());
        dir.write(
            "docs/a.md",
            "### [old-id]\n\n```claim\nkind: constraint\nevaluator: test\n```\n",
        );
        dir.write(
            "docs/b.md",
            "### [b]\n\n[a link so C5 is satisfied before the rename](old-id)\n\n```claim\nkind: constraint\nevaluator: test\ndepends: [\n  old-id\n]\n```\n",
        );
        let h = harness(&dir);

        // Confirm the premise directly: the multi-line entry really is
        // invisible to the site finder, in the exact file it lives in.
        let b_source = dir.read("docs/b.md");
        assert!(
            extract::find_rename_sites(&b_source, "old-id")
                .iter()
                .all(|s| s.kind != RenameSiteKind::Depends),
            "premise failed: the multi-line depends entry was unexpectedly located"
        );

        let err = plan_rename(
            dir.path(),
            &h.loaded,
            &h.config,
            &h.register_path,
            &h.markers,
            "old-id",
            "new-id",
        )
        .expect_err("a citation the writer cannot locate must be refused, not silently dropped");
        assert!(
            matches!(err, RenameError::InvariantDiverged(_)),
            "expected InvariantDiverged, got {err:?}"
        );
        if let RenameError::InvariantDiverged(diff) = &err {
            assert!(!diff.is_empty());
        }

        // Nothing was written — both files are byte-identical to what
        // they were before `plan_rename` ran.
        assert_eq!(
            dir.read("docs/a.md"),
            "### [old-id]\n\n```claim\nkind: constraint\nevaluator: test\n```\n"
        );
        assert_eq!(dir.read("docs/b.md"), b_source);
    }

    #[test]
    fn a_duplicate_id_definition_is_renamed_at_every_site() {
        // Not a case rename is expected to fix (C2 already flags a
        // duplicate id as a corpus defect); renaming must still touch
        // every site rather than silently picking one and leaving the
        // other stale.
        let dir = tempdir();
        dir.write("docket.ncl", basic_docket_ncl());
        dir.write(
            "docs/a.md",
            "### [old-id]\n\n```claim\nkind: constraint\nevaluator: test\n```\n",
        );
        dir.write(
            "docs/b.md",
            "### [old-id]\n\n```claim\nkind: constraint\nevaluator: test\n```\n",
        );
        let h = harness(&dir);
        // C2 (duplicate id) makes the register disagree with itself
        // after the id substitution in an unrelated way (two claims
        // collapsing onto one key), so this specific corpus is expected
        // to hit the invariant divergence path — confirming duplicates
        // are surfaced, not silently mis-renamed, is the point of this
        // test, not a clean success.
        let result = plan_rename(
            dir.path(),
            &h.loaded,
            &h.config,
            &h.register_path,
            &h.markers,
            "old-id",
            "new-id",
        );
        match result {
            Ok(plan) => {
                // If the register happens to treat this permissively,
                // both sites must still have been renamed.
                assert_eq!(plan.changes.len(), 2);
            }
            Err(RenameError::InvariantDiverged(_)) => {
                // Also acceptable: the corpus was already broken (C2),
                // and refusing rather than guessing is exactly this
                // tool's stated floor.
            }
            Err(other) => panic!("unexpected error: {other:?}"),
        }
    }
}
