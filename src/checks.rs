//! The five checks (MVP.md §3), plus `orphan-claim` (§1.1) and
//! `duplicate-stem` (§3's stem-uniqueness precondition).

use crate::config::Config;
use crate::contract::{self, ContractError};
use crate::corpus::LoadedCorpus;
use crate::model::{Claim, CiteRef, Document, Kind, anchor_matches};
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
    DuplicateStem,
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
            CheckId::DuplicateStem => "duplicate-stem",
        }
    }
}

/// MVP.md §3: "Each failure names the file, the line, and the offending
/// value." `line` is `None` only for `duplicate-stem`, which names a
/// *file* (there's no single offending line — the offense is that two
/// files share a stem).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    pub check: CheckId,
    pub file: String,
    pub line: Option<usize>,
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
            line: Some(orphan.line.0),
            message: "claim block has no preceding bracket-kebab id heading".to_string(),
        });
    }

    // duplicate-stem (§3 precondition): stems must be unique corpus-wide,
    // since document-anchor references depend on it.
    let mut by_stem: HashMap<&str, Vec<&Document>> = HashMap::new();
    for doc in &corpus.documents {
        by_stem.entry(doc.stem.as_str()).or_default().push(doc);
    }
    let mut ambiguous_stems: HashSet<&str> = HashSet::new();
    for (stem, docs) in &by_stem {
        if docs.len() > 1 {
            ambiguous_stems.insert(stem);
            for doc in docs {
                let others: Vec<&str> = docs
                    .iter()
                    .map(|d| d.file.as_str())
                    .filter(|f| *f != doc.file)
                    .collect();
                failures.push(Failure {
                    check: CheckId::DuplicateStem,
                    file: doc.file.clone(),
                    line: None,
                    message: format!("stem {stem:?} is also used by {}", others.join(", ")),
                });
            }
        }
    }

    // C1: every claim block validates against the Nickel contract.
    for claim in &corpus.claims {
        match contract::validate_claim_block(contract_path, &claim.raw.yaml)? {
            ContractCheck::Valid => {}
            ContractCheck::Violated { diagnostic } => failures.push(Failure {
                check: CheckId::C1,
                file: claim.file.clone(),
                line: Some(claim.block_line.0),
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
                    line: Some(claim.heading_line.0),
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
                line: Some(claim.block_line.0),
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
            if !resolves(cite, &claim_ids, &corpus.documents, &ambiguous_stems) {
                failures.push(Failure {
                    check: CheckId::C4,
                    file: claim.file.clone(),
                    line: Some(claim.block_line.0),
                    message: format!("cites target {cite} does not resolve"),
                });
            }
        }
    }

    // C5: prose links and cites agree, per claim, restricted to targets
    // that resolve (MVP.md §3).
    for claim in &corpus.claims {
        let prose_set: BTreeSet<String> = claim
            .prose_links
            .iter()
            .filter_map(|href| normalize_prose_link(href, &claim.file))
            .filter(|cite| resolves(cite, &claim_ids, &corpus.documents, &ambiguous_stems))
            .map(|c| c.to_string())
            .collect();
        let cites_set: BTreeSet<String> = claim.cites.iter().map(|c| c.to_string()).collect();

        if prose_set != cites_set {
            let only_prose: Vec<&String> = prose_set.difference(&cites_set).collect();
            let only_cites: Vec<&String> = cites_set.difference(&prose_set).collect();
            failures.push(Failure {
                check: CheckId::C5,
                file: claim.file.clone(),
                line: Some(claim.heading_line.0),
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
/// target resolves. A stem shared by more than one document (a
/// `duplicate-stem` violation) can't be resolved unambiguously, so any
/// reference through it is treated as unresolved here — consistent with
/// MVP.md §3's framing of stem-uniqueness as a precondition references
/// depend on.
fn resolves(
    cite: &CiteRef,
    claim_ids: &HashSet<&str>,
    documents: &[Document],
    ambiguous_stems: &HashSet<&str>,
) -> bool {
    match cite {
        CiteRef::Claim(id) => claim_ids.contains(id.as_str()),
        CiteRef::DocAnchor { stem, anchor } => {
            if ambiguous_stems.contains(stem.as_str()) {
                return false;
            }
            documents
                .iter()
                .filter(|d| d.stem == *stem)
                .any(|d| d.headings.iter().any(|h| anchor_matches(&h.text, anchor)))
        }
    }
}

/// Normalize a raw markdown link href (§3, C5's `L`) into the same
/// `<doc-stem>#<anchor>` / bare-claim-id vocabulary `cites` uses, so the
/// two sets can be compared. **Not specified by MVP.md** — §1.3 only
/// defines `cites`' own syntax; how a real markdown href (a relative
/// path, possibly `.md`-suffixed, possibly fragment-only) maps onto that
/// vocabulary is this crate's own judgment call, documented here rather
/// than silently assumed:
///
/// - A fragment-only href (`#6`) means "an anchor in this document" —
///   normalized against the *citing claim's own* document stem.
/// - A path with a `#anchor` (`../models/composition-model.md#6`)
///   normalizes to `<basename-without-extension>#anchor`, discarding the
///   directory — `cites` entries are stem-relative, not path-relative,
///   and stems are unique corpus-wide (`duplicate-stem`).
/// - A path with no anchor at all has no representation in `cites`'
///   syntax (there is no "whole document, no anchor" ref form), so it's
///   excluded from `L` rather than guessed at.
/// - A bare token with neither `/` nor `.` (`lock-groundness`) is treated
///   as a claim-id candidate, since that's the only `cites` shape it
///   could possibly match.
fn normalize_prose_link(href: &str, claiming_file: &str) -> Option<CiteRef> {
    let (path_part, anchor) = match href.split_once('#') {
        Some((p, a)) => (p, Some(a)),
        None => (href, None),
    };
    let anchor = anchor.filter(|a| !a.is_empty());

    if path_part.is_empty() {
        let stem = stem_of(claiming_file);
        return anchor.map(|a| CiteRef::DocAnchor {
            stem,
            anchor: a.to_string(),
        });
    }

    match anchor {
        Some(a) => Some(CiteRef::DocAnchor {
            stem: stem_of(path_part),
            anchor: a.to_string(),
        }),
        None => {
            if path_part.contains('/') || path_part.contains('.') {
                None
            } else {
                Some(CiteRef::Claim(path_part.to_string()))
            }
        }
    }
}

fn stem_of(path: &str) -> String {
    let base = path.rsplit('/').next().unwrap_or(path);
    match base.rsplit_once('.') {
        Some((stem, _ext)) => stem.to_string(),
        None => base.to_string(),
    }
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
        report.failures.iter().filter(|f| f.check == check).collect()
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
            "### [x]\n\n```claim\nkind: constraint\nevaluator: test\ncites: [composition-model#6]\n```\n",
        );
        let report = run(&dir);
        assert!(only(&report, CheckId::C4).is_empty(), "{:#?}", report.failures);
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
            "### [x]\n\n```claim\nkind: constraint\nevaluator: test\ncites: [m#6]\n```\n",
        );
        let report = run(&dir);
        assert_eq!(only(&report, CheckId::C4).len(), 1);
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
            "### [x]\n\nSee [the fact-set](../models/composition-model.md#6).\n\n```claim\nkind: constraint\nevaluator: test\ncites: [composition-model#6]\n```\n",
        );
        let report = run(&dir);
        assert!(only(&report, CheckId::C5).is_empty(), "{:#?}", report.failures);
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
            "### [x]\n\nNo links here.\n\n```claim\nkind: constraint\nevaluator: test\ncites: [composition-model#6]\n```\n",
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
            "### [x]\n\nSee [the web](https://example.com) and [an untracked file](../scratch/notes.md).\n\n```claim\nkind: constraint\nevaluator: test\ncites: []\n```\n",
        );
        let report = run(&dir);
        assert!(only(&report, CheckId::C5).is_empty(), "{:#?}", report.failures);
    }

    #[test]
    fn orphan_claim_is_reported() {
        let dir = tempdir();
        dir.write(
            "docket.ncl",
            r#"{ genres = [ { path = "docs/specs/**", kinds = ["constraint"] } ] }"#,
        );
        dir.write("docs/specs/x.md", "## Notes\n\n```claim\nkind: constraint\n```\n");
        let report = run(&dir);
        assert_eq!(only(&report, CheckId::OrphanClaim).len(), 1);
    }

    #[test]
    fn duplicate_stem_is_reported_for_both_files() {
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
        dir.write("docs/specs/shared.md", "# a\n");
        dir.write("docs/models/shared.md", "# b\n");
        let report = run(&dir);
        assert_eq!(only(&report, CheckId::DuplicateStem).len(), 2);
    }

    #[test]
    fn normalize_prose_link_handles_the_documented_shapes() {
        assert_eq!(
            normalize_prose_link("../models/composition-model.md#6", "docs/specs/x.md"),
            Some(CiteRef::DocAnchor {
                stem: "composition-model".into(),
                anchor: "6".into()
            })
        );
        assert_eq!(
            normalize_prose_link("#6", "docs/specs/x.md"),
            Some(CiteRef::DocAnchor {
                stem: "x".into(),
                anchor: "6".into()
            })
        );
        assert_eq!(
            normalize_prose_link("lock-groundness", "docs/specs/x.md"),
            Some(CiteRef::Claim("lock-groundness".into()))
        );
        assert_eq!(normalize_prose_link("composition-model.md", "docs/specs/x.md"), None);
    }
}
