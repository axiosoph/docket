//! The five checks (MVP.md §3), plus `orphan-claim` (§1.1). No
//! stem-uniqueness precondition exists: document identifiers are
//! corpus-relative paths (§1.3) and are therefore unique by construction.

use crate::config::Config;
use crate::contract::{self, ContractError};
use crate::corpus::LoadedCorpus;
use crate::model::{CiteRef, Claim, Document, Kind, anchor_matches};
use crate::nickel::ContractCheck;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckId {
    C1,
    C2,
    C3,
    C4,
    C5,
    OrphanClaim,
}

impl CheckId {
    pub fn as_str(&self) -> &'static str {
        match self {
            CheckId::C1 => "C1",
            CheckId::C2 => "C2",
            CheckId::C3 => "C3",
            CheckId::C4 => "C4",
            CheckId::C5 => "C5",
            CheckId::OrphanClaim => "orphan-claim",
        }
    }
}

/// MVP.md §3: "Each failure names the file, the line, and the offending
/// value."
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    pub check: CheckId,
    pub file: String,
    pub line: usize,
    pub message: String,
}

#[derive(Debug, Clone, Default)]
pub struct CheckReport {
    pub failures: Vec<Failure>,
}

impl CheckReport {
    pub fn passed(&self) -> bool {
        self.failures.is_empty()
    }
}

/// Run every check over an already-loaded corpus. `contract_path` is C1's
/// Nickel contract (see contract.rs for why its location is a documented
/// assumption rather than a spec fact).
pub fn run_checks(
    loaded: &LoadedCorpus,
    config: &Config,
    contract_path: &Path,
) -> Result<CheckReport, ContractError> {
    let corpus = &loaded.corpus;
    let mut failures = Vec::new();

    // orphan-claim (§1.1): a claim fence with no preceding bracket-kebab
    // heading. Extraction already found these; report them directly.
    for orphan in &loaded.orphan_claims {
        failures.push(Failure {
            check: CheckId::OrphanClaim,
            file: orphan.file.clone(),
            line: orphan.line.0,
            message: "claim block has no preceding bracket-kebab id heading".to_string(),
        });
    }

    // C1: every claim block validates against the Nickel contract.
    for claim in &corpus.claims {
        match contract::validate_claim_block(contract_path, &claim.raw.yaml)? {
            ContractCheck::Valid => {}
            ContractCheck::Violated { diagnostic } => failures.push(Failure {
                check: CheckId::C1,
                file: claim.file.clone(),
                line: claim.block_line.0,
                message: diagnostic,
            }),
        }
    }

    // C2: claim ids are unique corpus-wide.
    let mut by_id: HashMap<&str, Vec<&Claim>> = HashMap::new();
    for claim in &corpus.claims {
        by_id.entry(claim.id.as_str()).or_default().push(claim);
    }
    for (id, claims) in &by_id {
        if claims.len() > 1 {
            for claim in claims {
                let others: Vec<String> = claims
                    .iter()
                    .filter(|c| c.file != claim.file || c.heading_line != claim.heading_line)
                    .map(|c| format!("{}:{}", c.file, c.heading_line))
                    .collect();
                failures.push(Failure {
                    check: CheckId::C2,
                    file: claim.file.clone(),
                    line: claim.heading_line.0,
                    message: format!("id {id:?} is also declared at {}", others.join(", ")),
                });
            }
        }
    }

    // C3: kind is permitted by the genre of the claim's own file.
    for claim in &corpus.claims {
        let Some(doc) = corpus.documents.iter().find(|d| d.file == claim.file) else {
            continue; // extraction invariant: every claim comes from a scanned document
        };
        let Some(genre) = config.genres.iter().find(|g| g.path == doc.genre_path) else {
            continue;
        };
        // A missing/unrecognized kind is C1's finding, not C3's.
        let Some(kind) = claim.raw.kind.as_deref().and_then(parse_kind) else {
            continue;
        };
        if !genre.kinds.contains(&kind) {
            failures.push(Failure {
                check: CheckId::C3,
                file: claim.file.clone(),
                line: claim.block_line.0,
                message: format!(
                    "kind {:?} is not permitted by genre {:?} (permits {:?})",
                    kind.as_str(),
                    genre.path,
                    genre.kinds.iter().map(Kind::as_str).collect::<Vec<_>>()
                ),
            });
        }
    }

    let claim_ids: HashSet<&str> = corpus.claims.iter().map(|c| c.id.as_str()).collect();

    // C4: every cites target resolves.
    for claim in &corpus.claims {
        for cite in &claim.cites {
            if !resolves(cite, &claim_ids, &corpus.documents) {
                failures.push(Failure {
                    check: CheckId::C4,
                    file: claim.file.clone(),
                    line: claim.block_line.0,
                    message: format!("cites target {cite} does not resolve"),
                });
            }
        }
    }

    // C5: prose links and cites agree, per claim, restricted to targets
    // that are ref-shaped per §1.3 — a SYNTACTIC filter, not a resolution
    // filter (MVP.md §3's boxed note). Filtering by resolution instead
    // would make a dangling `cites` entry with a matching prose link fail
    // both C4 and C5 while lying in the C5 diagnostic: prose and cites
    // agree perfectly there (both name the same nonexistent target), so
    // reporting a divergence misdiagnoses it. C4 owns "does this exist";
    // C5 owns "do the two representations agree."
    for claim in &corpus.claims {
        let prose_set: BTreeSet<String> = claim
            .prose_links
            .iter()
            .filter_map(|href| normalize_prose_link(href, &claim.file))
            .filter(is_ref_shaped)
            .map(|c| c.to_string())
            .collect();
        let cites_set: BTreeSet<String> = claim.cites.iter().map(|c| c.to_string()).collect();

        if prose_set != cites_set {
            let only_prose: Vec<&String> = prose_set.difference(&cites_set).collect();
            let only_cites: Vec<&String> = cites_set.difference(&prose_set).collect();
            failures.push(Failure {
                check: CheckId::C5,
                file: claim.file.clone(),
                line: claim.heading_line.0,
                message: format!(
                    "prose links and cites disagree for {:?}: only in prose {only_prose:?}, only in cites {only_cites:?}",
                    claim.id
                ),
            });
        }
    }

    Ok(CheckReport { failures })
}

fn parse_kind(s: &str) -> Option<Kind> {
    match s {
        "requirement" => Some(Kind::Requirement),
        "invariant" => Some(Kind::Invariant),
        "constraint" => Some(Kind::Constraint),
        _ => None,
    }
}

/// Whether a (well-formed, per C1) `cites` or normalized-prose-link
/// target resolves. Document identifiers are corpus-relative paths and
/// therefore unique by construction (MVP.md §1.3), so — unlike the
/// retired `duplicate-stem` era — a single `.find()` is enough; there is
/// no ambiguity to guard against here.
fn resolves(cite: &CiteRef, claim_ids: &HashSet<&str>, documents: &[Document]) -> bool {
    match cite {
        CiteRef::Claim(id) => claim_ids.contains(id.as_str()),
        CiteRef::DocAnchor { path, anchor } => documents
            .iter()
            .find(|d| d.doc_path == *path)
            .is_some_and(|d| d.headings.iter().any(|h| anchor_matches(&h.text, anchor))),
    }
}

/// Whether a normalized prose-link target is **ref-shaped** per §1.3 —
/// C5's filter, syntactic only: no corpus lookup, so an entry with a
/// dangling but well-formed target still counts toward `L`. Mirrors
/// `contracts/claim.ncl`'s `Ref` predicate (`ClaimIdPattern` /
/// `DocRefPattern`), which is the shape C1 already enforces on `cites`
/// itself.
fn is_ref_shaped(cite: &CiteRef) -> bool {
    match cite {
        CiteRef::Claim(id) => is_kebab_case(id),
        CiteRef::DocAnchor { path, anchor } => !path.is_empty() && !anchor.is_empty(),
    }
}

fn is_kebab_case(s: &str) -> bool {
    !s.is_empty()
        && s.split('-').all(|seg| {
            !seg.is_empty()
                && seg
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        })
}

/// Normalize a raw markdown link href (§3, C5's `L`) into the same
/// `<doc-path>#<anchor>` / bare-claim-id vocabulary `cites` uses, so the
/// two sets can be compared. Per MVP.md §1.3's "Prose-link normalization":
///
/// - A fragment-only href (`#6`) refers to the containing document —
///   normalized against the *citing claim's own file*, minus `.md`.
/// - Otherwise, the href's path is **resolved against the containing
///   file's directory** (real relative-link semantics: `../models/x.md`
///   written from `docs/specs/` means `docs/models/x.md`) to get a
///   corpus-relative path, which is then stripped of its `.md` extension.
///   A path that resolves above the corpus root is not ref-shaped and is
///   dropped (`resolve_relative` returns `None`).
/// - A path with no anchor at all has no representation in `cites`'
///   syntax (there is no "whole document, no anchor" ref form), so it's
///   excluded from `L` rather than guessed at.
/// - A bare token with no anchor, no `/`, and no `.md` suffix can only be
///   a claim-id reference: every corpus document has a `.md` extension
///   (§2), so a same-directory reference lacking both an anchor and that
///   extension can't name a document. This check runs *before* path
///   resolution specifically so a claim id (which has no path shape at
///   all) is never accidentally joined onto a directory.
fn normalize_prose_link(href: &str, claiming_file: &str) -> Option<CiteRef> {
    let (path_part, anchor) = match href.split_once('#') {
        Some((p, a)) => (p, Some(a)),
        None => (href, None),
    };
    let anchor = anchor.filter(|a| !a.is_empty());

    if path_part.is_empty() {
        let path = strip_md_extension(claiming_file);
        return anchor.map(|a| CiteRef::DocAnchor {
            path,
            anchor: a.to_string(),
        });
    }

    if anchor.is_none() && !path_part.contains('/') && !path_part.ends_with(".md") {
        return Some(CiteRef::Claim(path_part.to_string()));
    }

    let resolved = resolve_relative(dir_of(claiming_file), path_part)?;

    anchor.map(|a| CiteRef::DocAnchor {
        path: strip_md_extension(&resolved),
        anchor: a.to_string(),
    })
}

fn dir_of(path: &str) -> &str {
    match path.rsplit_once('/') {
        Some((dir, _file)) => dir,
        None => "",
    }
}

fn strip_md_extension(path: &str) -> String {
    path.strip_suffix(".md").unwrap_or(path).to_string()
}

/// Lexically resolve `relative` against `base_dir` — `.` and empty
/// segments are dropped, `..` pops the last pushed segment. Returns
/// `None` if a `..` would need to pop past the corpus root (MVP.md
/// §1.3: "A link that escapes the corpus root is not ref-shaped and is
/// ignored"). A leading `/` in `relative` is treated as corpus-root-relative
/// rather than joined onto `base_dir` — not addressed by MVP.md's worked
/// example, but the natural reading of an absolute-style link in a corpus
/// that has no filesystem root of its own to escape to.
fn resolve_relative(base_dir: &str, relative: &str) -> Option<String> {
    let (base_dir, relative) = match relative.strip_prefix('/') {
        Some(root_relative) => ("", root_relative),
        None => (base_dir, relative),
    };
    let mut parts: Vec<&str> = if base_dir.is_empty() {
        Vec::new()
    } else {
        base_dir.split('/').collect()
    };
    for seg in relative.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            other => parts.push(other),
        }
    }
    Some(parts.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::load_config;
    use crate::corpus::load_corpus;
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
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn tempdir() -> TempDir {
        let dir = std::env::temp_dir().join(format!(
            "docket-checks-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }

    const TEST_CONTRACT: &str = r#"
{
  kind | std.contract.from_predicate (fun v => std.array.elem v ["requirement", "invariant", "constraint"]),
  evaluator | std.contract.from_predicate (fun v => std.array.elem v ["proof", "model-check", "property-test", "test", "example", "none"]),
  cites | Array String | default = [],
}
"#;

    fn write_test_contract(dir: &TempDir) -> std::path::PathBuf {
        dir.write("contracts/claim.ncl", TEST_CONTRACT);
        dir.path().join("contracts/claim.ncl")
    }

    fn run(dir: &TempDir) -> CheckReport {
        let config = load_config(dir.path()).unwrap();
        let loaded = load_corpus(dir.path(), &config).unwrap();
        let contract = write_test_contract(dir);
        run_checks(&loaded, &config, &contract).unwrap()
    }

    fn only(report: &CheckReport, check: CheckId) -> Vec<&Failure> {
        report
            .failures
            .iter()
            .filter(|f| f.check == check)
            .collect()
    }

    #[test]
    fn a_clean_corpus_passes_every_check() {
        let dir = tempdir();
        dir.write(
            "docket.ncl",
            r#"{ genres = [ { path = "docs/specs/**", kinds = ["constraint"] } ] }"#,
        );
        dir.write(
            "docs/specs/lock.md",
            "### [lock-groundness]\n\nEvery lock value MUST be ground.\n\n```claim\nkind: constraint\nevaluator: test\ncites: []\n```\n",
        );
        let report = run(&dir);
        assert!(report.passed(), "{:#?}", report.failures);
    }

    #[test]
    fn c1_fails_on_an_unknown_field() {
        let dir = tempdir();
        dir.write(
            "docket.ncl",
            r#"{ genres = [ { path = "docs/specs/**", kinds = ["constraint"] } ] }"#,
        );
        dir.write(
            "docs/specs/x.md",
            "### [x]\n\n```claim\nkind: constraint\nevaluator: test\ntypo: oops\n```\n",
        );
        let report = run(&dir);
        assert_eq!(only(&report, CheckId::C1).len(), 1);
    }

    #[test]
    fn c2_fails_on_duplicate_ids() {
        let dir = tempdir();
        dir.write(
            "docket.ncl",
            r#"{ genres = [ { path = "docs/specs/**", kinds = ["constraint"] } ] }"#,
        );
        dir.write(
            "docs/specs/a.md",
            "### [dup]\n\n```claim\nkind: constraint\nevaluator: test\n```\n",
        );
        dir.write(
            "docs/specs/b.md",
            "### [dup]\n\n```claim\nkind: constraint\nevaluator: test\n```\n",
        );
        let report = run(&dir);
        assert_eq!(only(&report, CheckId::C2).len(), 2);
    }

    #[test]
    fn c3_fails_when_kind_is_not_permitted_by_genre() {
        let dir = tempdir();
        dir.write(
            "docket.ncl",
            r#"{ genres = [ { path = "docs/adr/**", kinds = [] } ] }"#,
        );
        dir.write(
            "docs/adr/decision.md",
            "### [normative-leak]\n\n```claim\nkind: constraint\nevaluator: test\n```\n",
        );
        let report = run(&dir);
        assert_eq!(only(&report, CheckId::C3).len(), 1);
    }

    #[test]
    fn c3_passes_when_kind_is_permitted() {
        let dir = tempdir();
        dir.write(
            "docket.ncl",
            r#"{ genres = [ { path = "docs/models/**", kinds = ["invariant"] } ] }"#,
        );
        dir.write(
            "docs/models/m.md",
            "### [ok]\n\n```claim\nkind: invariant\nevaluator: test\n```\n",
        );
        let report = run(&dir);
        assert!(only(&report, CheckId::C3).is_empty());
    }

    #[test]
    fn c4_fails_on_a_dangling_claim_id_cite() {
        let dir = tempdir();
        dir.write(
            "docket.ncl",
            r#"{ genres = [ { path = "docs/specs/**", kinds = ["constraint"] } ] }"#,
        );
        dir.write(
            "docs/specs/x.md",
            "### [x]\n\n```claim\nkind: constraint\nevaluator: test\ncites: [does-not-exist]\n```\n",
        );
        let report = run(&dir);
        assert_eq!(only(&report, CheckId::C4).len(), 1);
    }

    #[test]
    fn a_dangling_cite_with_a_matching_prose_link_fails_only_c4_not_c5() {
        // The exact case MVP.md §3's boxed note calls out: prose and
        // cites AGREE (both name the same nonexistent target), so only
        // C4 ("this target does not exist") should fire — a resolution
        // filter on C5's L would incorrectly also fire C5 here, with a
        // diagnostic that lies about a divergence that doesn't exist.
        let dir = tempdir();
        dir.write(
            "docket.ncl",
            r#"{ genres = [ { path = "docs/specs/**", kinds = ["constraint"] } ] }"#,
        );
        dir.write(
            "docs/specs/x.md",
            "### [x]\n\nSee [related work](does-not-exist).\n\n```claim\nkind: constraint\nevaluator: test\ncites: [does-not-exist]\n```\n",
        );
        let report = run(&dir);
        assert_eq!(only(&report, CheckId::C4).len(), 1);
        assert!(
            only(&report, CheckId::C5).is_empty(),
            "{:#?}",
            report.failures
        );
    }

    #[test]
    fn c4_resolves_a_numbered_document_anchor() {
        let dir = tempdir();
        dir.write(
            "docket.ncl",
            r#"{
              genres = [
                { path = "docs/specs/**", kinds = ["constraint"] },
                { path = "docs/models/**", kinds = ["invariant"] },
              ],
            }"#,
        );
        dir.write(
            "docs/models/composition-model.md",
            "## 6. The fact-set: the substrate's only state\n\nprose\n",
        );
        dir.write(
            "docs/specs/x.md",
            "### [x]\n\n```claim\nkind: constraint\nevaluator: test\ncites: [docs/models/composition-model#6]\n```\n",
        );
        let report = run(&dir);
        assert!(
            only(&report, CheckId::C4).is_empty(),
            "{:#?}",
            report.failures
        );
    }

    #[test]
    fn c4_rejects_a_numeric_prefix_collision() {
        let dir = tempdir();
        dir.write(
            "docket.ncl",
            r#"{
              genres = [
                { path = "docs/specs/**", kinds = ["constraint"] },
                { path = "docs/models/**", kinds = ["invariant"] },
              ],
            }"#,
        );
        dir.write("docs/models/m.md", "## 60. Something else\n");
        dir.write(
            "docs/specs/x.md",
            "### [x]\n\n```claim\nkind: constraint\nevaluator: test\ncites: [docs/models/m#6]\n```\n",
        );
        let report = run(&dir);
        assert_eq!(only(&report, CheckId::C4).len(), 1);
    }

    #[test]
    fn c4_two_documents_sharing_a_basename_resolve_independently() {
        // The exact real-corpus shape that retired duplicate-stem: three
        // README.md files under one genre, indistinguishable by
        // basename. Both must resolve correctly by their own path.
        let dir = tempdir();
        dir.write(
            "docket.ncl",
            r#"{
              genres = [
                { path = "docs/specs/**", kinds = ["constraint"] },
                { path = "docs/models/**", kinds = ["invariant"] },
              ],
            }"#,
        );
        dir.write("docs/models/lean/README.md", "## 1. Lean notes\n");
        dir.write("docs/models/tla/README.md", "## 1. TLA notes\n");
        dir.write(
            "docs/specs/x.md",
            "### [x]\n\n```claim\nkind: constraint\nevaluator: test\ncites: [docs/models/lean/README#1, docs/models/tla/README#1]\n```\n",
        );
        let report = run(&dir);
        assert!(
            only(&report, CheckId::C4).is_empty(),
            "{:#?}",
            report.failures
        );
    }

    #[test]
    fn c5_passes_when_prose_link_and_cites_agree() {
        let dir = tempdir();
        dir.write(
            "docket.ncl",
            r#"{
              genres = [
                { path = "docs/specs/**", kinds = ["constraint"] },
                { path = "docs/models/**", kinds = ["invariant"] },
              ],
            }"#,
        );
        dir.write("docs/models/composition-model.md", "## 6. The fact-set\n");
        dir.write(
            "docs/specs/x.md",
            "### [x]\n\nSee [the fact-set](../models/composition-model.md#6).\n\n```claim\nkind: constraint\nevaluator: test\ncites: [docs/models/composition-model#6]\n```\n",
        );
        let report = run(&dir);
        assert!(
            only(&report, CheckId::C5).is_empty(),
            "{:#?}",
            report.failures
        );
    }

    #[test]
    fn c5_fails_when_cites_has_an_entry_prose_never_links() {
        let dir = tempdir();
        dir.write(
            "docket.ncl",
            r#"{
              genres = [
                { path = "docs/specs/**", kinds = ["constraint"] },
                { path = "docs/models/**", kinds = ["invariant"] },
              ],
            }"#,
        );
        dir.write("docs/models/composition-model.md", "## 6. The fact-set\n");
        dir.write(
            "docs/specs/x.md",
            "### [x]\n\nNo links here.\n\n```claim\nkind: constraint\nevaluator: test\ncites: [docs/models/composition-model#6]\n```\n",
        );
        let report = run(&dir);
        assert_eq!(only(&report, CheckId::C5).len(), 1);
    }

    #[test]
    fn c5_ignores_an_external_url_and_a_link_outside_the_corpus() {
        let dir = tempdir();
        dir.write(
            "docket.ncl",
            r#"{ genres = [ { path = "docs/specs/**", kinds = ["constraint"] } ] }"#,
        );
        dir.write(
            "docs/specs/x.md",
            "### [x]\n\nSee [the web](https://example.com) and [escaping the root](../../outside.md).\n\n```claim\nkind: constraint\nevaluator: test\ncites: []\n```\n",
        );
        let report = run(&dir);
        assert!(
            only(&report, CheckId::C5).is_empty(),
            "{:#?}",
            report.failures
        );
    }

    #[test]
    fn orphan_claim_is_reported() {
        let dir = tempdir();
        dir.write(
            "docket.ncl",
            r#"{ genres = [ { path = "docs/specs/**", kinds = ["constraint"] } ] }"#,
        );
        dir.write(
            "docs/specs/x.md",
            "## Notes\n\n```claim\nkind: constraint\n```\n",
        );
        let report = run(&dir);
        assert_eq!(only(&report, CheckId::OrphanClaim).len(), 1);
    }

    #[test]
    fn normalize_prose_link_resolves_a_relative_path_against_the_citing_directory() {
        assert_eq!(
            normalize_prose_link("../models/composition-model.md#6", "docs/specs/x.md"),
            Some(CiteRef::DocAnchor {
                path: "docs/models/composition-model".into(),
                anchor: "6".into()
            })
        );
    }

    #[test]
    fn normalize_prose_link_fragment_only_means_the_containing_document() {
        assert_eq!(
            normalize_prose_link("#6", "docs/specs/x.md"),
            Some(CiteRef::DocAnchor {
                path: "docs/specs/x".into(),
                anchor: "6".into()
            })
        );
    }

    #[test]
    fn normalize_prose_link_bare_token_with_no_anchor_is_a_claim_id_candidate() {
        assert_eq!(
            normalize_prose_link("lock-groundness", "docs/specs/x.md"),
            Some(CiteRef::Claim("lock-groundness".into()))
        );
    }

    #[test]
    fn normalize_prose_link_bare_token_with_anchor_resolves_in_the_same_directory() {
        // No `/`, but an anchor is present, so this is a document-anchor
        // reference resolved relative to x.md's own directory — not a
        // claim id (claim ids never carry a `#`).
        assert_eq!(
            normalize_prose_link("sibling#3", "docs/specs/x.md"),
            Some(CiteRef::DocAnchor {
                path: "docs/specs/sibling".into(),
                anchor: "3".into()
            })
        );
    }

    #[test]
    fn normalize_prose_link_whole_document_with_no_anchor_is_unrepresentable() {
        assert_eq!(
            normalize_prose_link("composition-model.md", "docs/specs/x.md"),
            None
        );
    }

    #[test]
    fn normalize_prose_link_escaping_the_corpus_root_is_dropped() {
        assert_eq!(
            normalize_prose_link("../../../etc/passwd#1", "docs/specs/x.md"),
            None
        );
    }

    #[test]
    fn normalize_prose_link_leading_slash_is_corpus_root_relative() {
        // Not addressed by MVP.md's worked example — this crate's own
        // judgment call, documented on resolve_relative: an
        // absolute-style href is root-relative rather than joined onto
        // the citing file's directory.
        assert_eq!(
            normalize_prose_link(
                "/docs/models/composition-model.md#6",
                "docs/specs/deep/nested/x.md"
            ),
            Some(CiteRef::DocAnchor {
                path: "docs/models/composition-model".into(),
                anchor: "6".into()
            })
        );
    }
}
