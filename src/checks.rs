//! The five checks (MVP.md §3), plus `orphan-claim` (§1.1) and
//! `orphaned-because` (reference kinds R1, below). No stem-uniqueness
//! precondition exists: document identifiers are corpus-relative paths
//! (§1.3) and are therefore unique by construction.
//!
//! **Reference kinds** (`.ledger/2026-07-30-reference-kinds-and-document-resolution.md`,
//! R1): a `depends`/`because` entry is no longer one undifferentiated
//! `cites`. A dangling `depends` means the claim is broken — C4, `Fail`
//! severity. A dangling `because` means the claim still stands but its
//! stated reason is orphaned — a **distinct, lower-severity** result
//! (`orphaned-because`, `Warn`), because the two remedies are different
//! (R1) and collapsing them back into one severity would either block
//! merges on a merely-thin justification or silently swallow real
//! breakage. A **bare** reference (an undeclared prose link) is checked
//! by neither: it is never a `CiteRef` in the first place
//! ([`crate::model::Claim::refs`]), so there is nothing to resolve.

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
    /// A genre whose `kinds` is empty permits no RFC-2119 keyword in a
    /// scanned document's own-voice text (MVP.md §2/§3). Its own
    /// identifier rather than an extension of C3: C3 judges a
    /// *registered* claim's `kind` against its genre, while this judges
    /// text that carries no claim block at all — a disjoint failure mode
    /// C3's diagnostic ("kind X not permitted") cannot honestly describe.
    /// Named descriptively rather than numbered, the same convention
    /// `orphan-claim` already set for a check beyond MVP.md's five.
    NormativeProse,
    /// A dangling `because` target (reference kinds R1). Named and
    /// numbered independently of C4 for the same reason `NormativeProse`
    /// is independent of C3: C4's diagnostic ("this claim is broken")
    /// would be a lie here — the claim still stands, only its stated
    /// reason is gone. Always `Warn` severity; see [`Severity`].
    OrphanedBecause,
    /// A recognized id definition (heading- or bold-form, extract.rs)
    /// with no claim block — the coverage-count deliverable ("bold form
    /// recognition" dispatch, §2). Reported as a diagnostic rather than a
    /// bespoke subcommand: the (file, line, id) shape a definition site
    /// needs is exactly what `Diagnostic` already carries, and a corpus
    /// with hundreds of these is the normal starting state (dispatch),
    /// never a reason to fail — always `Warn` severity.
    UnregisteredDefinition,
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
            CheckId::NormativeProse => "normative-prose",
            CheckId::OrphanedBecause => "orphaned-because",
            CheckId::UnregisteredDefinition => "unregistered-definition",
        }
    }
}

/// R1's severity split, made structural rather than left to a message
/// string a caller could ignore: `Fail` is what MVP.md §5's exit code 1
/// means ("one or more checks failed"); `Warn` is reported the same way
/// but never flips the exit code — the same "reported, never failed on"
/// treatment README.md already gives an evaluator that discharges no
/// claim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Fail,
    Warn,
}

/// MVP.md §3: "Each failure names the file, the line, and the offending
/// value." Despite the name, a `Diagnostic` is not always a failure —
/// `severity` says which; kept as one type (rather than two parallel
/// vectors) so every check pushes into one place and `CheckReport`
/// doesn't have to merge two collections back into report order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub check: CheckId,
    pub severity: Severity,
    pub file: String,
    pub line: usize,
    pub message: String,
}

#[derive(Debug, Clone, Default)]
pub struct CheckReport {
    pub diagnostics: Vec<Diagnostic>,
}

impl CheckReport {
    /// True iff nothing at `Fail` severity fired — a report holding only
    /// `Warn`-severity diagnostics still passes (MVP.md §5, exit 0).
    pub fn passed(&self) -> bool {
        !self
            .diagnostics
            .iter()
            .any(|d| d.severity == Severity::Fail)
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
    let mut diagnostics = Vec::new();

    // orphan-claim (§1.1): a claim fence with no preceding bracket-kebab
    // heading. Extraction already found these; report them directly.
    for orphan in &loaded.orphan_claims {
        diagnostics.push(Diagnostic {
            check: CheckId::OrphanClaim,
            severity: Severity::Fail,
            file: orphan.file.clone(),
            line: orphan.line.0,
            message: "claim block has no preceding bracket-kebab id heading".to_string(),
        });
    }

    // C1: every claim block validates against the Nickel contract.
    for claim in &corpus.claims {
        match contract::validate_claim_block(contract_path, &claim.raw.yaml)? {
            ContractCheck::Valid => {}
            ContractCheck::Violated { diagnostic } => diagnostics.push(Diagnostic {
                check: CheckId::C1,
                severity: Severity::Fail,
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
                diagnostics.push(Diagnostic {
                    check: CheckId::C2,
                    severity: Severity::Fail,
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
            diagnostics.push(Diagnostic {
                check: CheckId::C3,
                severity: Severity::Fail,
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

    // C4: every `depends` target resolves. Dangling ⇒ the claim is
    // broken (reference kinds R1) — `Fail` severity, MVP.md §5 exit 1.
    for claim in &corpus.claims {
        for cite in &claim.depends {
            if !resolves(cite, &claim_ids, &corpus.documents) {
                diagnostics.push(Diagnostic {
                    check: CheckId::C4,
                    severity: Severity::Fail,
                    file: claim.file.clone(),
                    line: claim.block_line.0,
                    message: format!(
                        "depends target {cite} does not resolve — this claim is broken: rewire or remove the dependency"
                    ),
                });
            }
        }
    }

    // orphaned-because: every `because` target resolves, at `Warn`
    // severity — the claim still stands, only its stated reason is gone
    // (R1). Independent of C4 for the same reason `normative-prose` is
    // independent of C3: C4's "this claim is broken" would misdescribe
    // this case entirely, not merely under-report its severity.
    for claim in &corpus.claims {
        for cite in &claim.because {
            if !resolves(cite, &claim_ids, &corpus.documents) {
                diagnostics.push(Diagnostic {
                    check: CheckId::OrphanedBecause,
                    severity: Severity::Warn,
                    file: claim.file.clone(),
                    line: claim.block_line.0,
                    message: format!(
                        "because target {cite} does not resolve — the claim's stated reason is orphaned, not the claim itself: restate the reason, or confirm the claim was vestigial"
                    ),
                });
            }
        }
    }

    // C5 (replaced rule — reference kinds R3): every `depends` and
    // `because` entry carries a prose link; a prose link that is not
    // declared is a bare reference and asserts nothing, so it is not
    // required to appear here at all. The requirement is one-directional
    // — declared ⊆ prose, not the set equality the original C5 required
    // — restricted to prose targets that are ref-shaped per §1.3, a
    // SYNTACTIC filter, not a resolution filter (MVP.md §3's boxed note,
    // preserved unchanged by this replacement): a dangling `depends`
    // entry with a matching prose link still names the same nonexistent
    // target either way, so it fails C4 alone, not C4 and C5 both.
    for claim in &corpus.claims {
        let prose_set: BTreeSet<CiteRef> = claim
            .prose_links
            .iter()
            .filter_map(|href| normalize_prose_link(href, &claim.file))
            .filter(is_ref_shaped)
            .collect();

        let undeclared: Vec<String> = claim
            .refs()
            .filter(|(_, cite)| !prose_link_declares(cite, &prose_set))
            .map(|(kind, cite)| format!("{}:{}", kind.as_str(), cite))
            .collect();

        if !undeclared.is_empty() {
            diagnostics.push(Diagnostic {
                check: CheckId::C5,
                severity: Severity::Fail,
                file: claim.file.clone(),
                line: claim.heading_line.0,
                message: format!(
                    "{:?} declares a dependence or reason with no matching prose link: {undeclared:?}",
                    claim.id
                ),
            });
        }
    }

    // normative-prose: a genre declaring `kinds = []` already permits no
    // claim blocks (MVP.md §2) — which means nothing normatively binding
    // may live in it. An RFC-2119 keyword in such a genre's own-voice
    // text is a binding assertion that carries no block, which is
    // exactly how it evades C3: C3 only ever sees a claim that exists.
    // Derived from `kinds`, not a new config field — a genre that permits
    // at least one kind is unaffected, since its job is to say MUST.
    // Extraction (extract.rs) finds every occurrence genre-agnostically;
    // only here, once genres are in view, is "kinds = []" known.
    for occurrence in &loaded.normative_occurrences {
        let Some(doc) = corpus.documents.iter().find(|d| d.file == occurrence.file) else {
            continue; // extraction invariant: every occurrence comes from a scanned document
        };
        let Some(genre) = config.genres.iter().find(|g| g.path == doc.genre_path) else {
            continue;
        };
        if genre.kinds.is_empty() {
            diagnostics.push(Diagnostic {
                check: CheckId::NormativeProse,
                severity: Severity::Fail,
                file: occurrence.file.clone(),
                line: occurrence.line.0,
                message: format!(
                    "normative keyword {:?} in own-voice prose, but genre {:?} permits no claim kinds",
                    occurrence.keyword, genre.path
                ),
            });
        }
    }

    // unregistered-definition: the coverage count (dispatch §2). Every
    // recognized definition (heading- or bold-form) that no claim block
    // adopted, reported at `Warn` severity — the normal starting state
    // for a real corpus, never a reason to flip the exit code.
    for def in &loaded.unregistered_definitions {
        diagnostics.push(Diagnostic {
            check: CheckId::UnregisteredDefinition,
            severity: Severity::Warn,
            file: def.file.clone(),
            line: def.line.0,
            message: format!("definition {:?} has no claim block — unregistered", def.id),
        });
    }

    Ok(CheckReport { diagnostics })
}

fn parse_kind(s: &str) -> Option<Kind> {
    match s {
        "requirement" => Some(Kind::Requirement),
        "invariant" => Some(Kind::Invariant),
        "constraint" => Some(Kind::Constraint),
        _ => None,
    }
}

/// Whether a (well-formed, per C1) `depends`/`because` or
/// normalized-prose-link target resolves. Document identifiers are
/// corpus-relative paths and
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

/// Whether a declared `depends`/`because` target is covered by some
/// entry in the claim's normalized prose-link set (C5's `D ⊆ L`).
///
/// A doc-anchor declaration requires an exact match — it already carries
/// its own path and anchor, so nothing else could represent it. A
/// claim-id declaration is satisfied two ways: an exact bare-id match
/// (`[…](spine-chain-complete)`), or any doc-anchor prose link whose
/// **anchor component** equals the id (`[…](#spine-chain-complete)` or
/// `[…](docs/x.md#spine-chain-complete)`) — the form every markdown
/// renderer and link checker already understands, and what an author
/// writes unprompted. The bare form stays accepted rather than retired:
/// it costs nothing to keep, and this repository's own MVP.md claims
/// (§1.3, §4.1) already use it (`.ledger/2026-07-30-claim-id-prose-link-breaks-link-checkers.md`).
fn prose_link_declares(cite: &CiteRef, prose: &BTreeSet<CiteRef>) -> bool {
    if prose.contains(cite) {
        return true;
    }
    match cite {
        CiteRef::Claim(id) => prose
            .iter()
            .any(|p| matches!(p, CiteRef::DocAnchor { anchor, .. } if anchor == id)),
        CiteRef::DocAnchor { .. } => false,
    }
}

/// Whether a normalized prose-link target is **ref-shaped** per §1.3 —
/// C5's filter, syntactic only: no corpus lookup, so an entry with a
/// dangling but well-formed target still counts toward `L`. Mirrors
/// `contracts/claim.ncl`'s `Ref` predicate (`ClaimIdPattern` /
/// `DocRefPattern`), which is the shape C1 already enforces on
/// `depends`/`because` entries themselves.
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
/// `<doc-path>#<anchor>` / bare-claim-id vocabulary `depends`/`because`
/// entries use, so a claim's declared refs can be checked against it. Per
/// MVP.md §1.3's "Prose-link normalization":
///
/// - A fragment-only href (`#6`) refers to the containing document —
///   normalized against the *citing claim's own file*, minus `.md`.
/// - Otherwise, the href's path is **resolved against the containing
///   file's directory** (real relative-link semantics: `../models/x.md`
///   written from `docs/specs/` means `docs/models/x.md`) to get a
///   corpus-relative path, which is then stripped of its `.md` extension.
///   A path that resolves above the corpus root is not ref-shaped and is
///   dropped (`resolve_relative` returns `None`).
/// - A path with no anchor at all has no representation in the ref
///   syntax `depends`/`because` share (there is no "whole document, no
///   anchor" ref form), so it's excluded from `L` rather than guessed at.
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
  depends | Array String | default = [],
  because | Array String | default = [],
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

    fn only(report: &CheckReport, check: CheckId) -> Vec<&Diagnostic> {
        report
            .diagnostics
            .iter()
            .filter(|f| f.check == check)
            .collect()
    }

    #[test]
    fn a_clean_corpus_passes_every_check() {
        let dir = tempdir();
        dir.write(
            "docket.ncl",
            r#"{ genres = [ { path = "docs/specs/**", kinds = ["constraint"], quadrant = "reference" } ] }"#,
        );
        dir.write(
            "docs/specs/lock.md",
            "### [lock-groundness]\n\nEvery lock value MUST be ground.\n\n```claim\nkind: constraint\nevaluator: test\n```\n",
        );
        let report = run(&dir);
        assert!(report.passed(), "{:#?}", report.diagnostics);
    }

    #[test]
    fn c1_fails_on_an_unknown_field() {
        let dir = tempdir();
        dir.write(
            "docket.ncl",
            r#"{ genres = [ { path = "docs/specs/**", kinds = ["constraint"], quadrant = "reference" } ] }"#,
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
            r#"{ genres = [ { path = "docs/specs/**", kinds = ["constraint"], quadrant = "reference" } ] }"#,
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
            r#"{ genres = [ { path = "docs/adr/**", kinds = [], quadrant = "explanation" } ] }"#,
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
            r#"{ genres = [ { path = "docs/models/**", kinds = ["invariant"], quadrant = "reference" } ] }"#,
        );
        dir.write(
            "docs/models/m.md",
            "### [ok]\n\n```claim\nkind: invariant\nevaluator: test\n```\n",
        );
        let report = run(&dir);
        assert!(only(&report, CheckId::C3).is_empty());
    }

    #[test]
    fn c4_fails_on_a_dangling_claim_id_depends() {
        let dir = tempdir();
        dir.write(
            "docket.ncl",
            r#"{ genres = [ { path = "docs/specs/**", kinds = ["constraint"], quadrant = "reference" } ] }"#,
        );
        dir.write(
            "docs/specs/x.md",
            "### [x]\n\n```claim\nkind: constraint\nevaluator: test\ndepends: [does-not-exist]\n```\n",
        );
        let report = run(&dir);
        let failures = only(&report, CheckId::C4);
        assert_eq!(failures.len(), 1);
        // Criterion 2: the message says the claim is broken, not merely
        // "does not resolve" — the severity split (R1) is only real if a
        // reader can tell the two remedies apart from the text alone.
        assert!(
            failures[0].message.contains("broken"),
            "{:?}",
            failures[0].message
        );
        assert_eq!(failures[0].severity, Severity::Fail);
        assert!(!report.passed());
    }

    #[test]
    fn a_dangling_depends_with_a_matching_prose_link_fails_only_c4_not_c5() {
        // The exact case MVP.md §3's boxed note calls out: prose and
        // depends AGREE (both name the same nonexistent target), so only
        // C4 ("this target does not exist") should fire — a resolution
        // filter on C5's L would incorrectly also fire C5 here, with a
        // diagnostic that lies about a divergence that doesn't exist.
        let dir = tempdir();
        dir.write(
            "docket.ncl",
            r#"{ genres = [ { path = "docs/specs/**", kinds = ["constraint"], quadrant = "reference" } ] }"#,
        );
        dir.write(
            "docs/specs/x.md",
            "### [x]\n\nSee [related work](does-not-exist).\n\n```claim\nkind: constraint\nevaluator: test\ndepends: [does-not-exist]\n```\n",
        );
        let report = run(&dir);
        assert_eq!(only(&report, CheckId::C4).len(), 1);
        assert!(
            only(&report, CheckId::C5).is_empty(),
            "{:#?}",
            report.diagnostics
        );
    }

    #[test]
    fn orphaned_because_fails_with_warn_severity_and_a_distinct_message() {
        // Criterion 3: a dangling `because` target is a DISTINCT,
        // lower-severity result from C4 — the claim stands, only its
        // stated reason is orphaned. `report.passed()` must stay true: a
        // `Warn`-only report never flips MVP.md §5's exit code.
        let dir = tempdir();
        dir.write(
            "docket.ncl",
            r#"{ genres = [ { path = "docs/specs/**", kinds = ["constraint"], quadrant = "reference" } ] }"#,
        );
        dir.write(
            "docs/specs/x.md",
            "### [x]\n\nSee [the old reason](does-not-exist).\n\n```claim\nkind: constraint\nevaluator: test\nbecause: [does-not-exist]\n```\n",
        );
        let report = run(&dir);
        let warnings = only(&report, CheckId::OrphanedBecause);
        assert_eq!(warnings.len(), 1, "{:#?}", report.diagnostics);
        assert_eq!(warnings[0].severity, Severity::Warn);
        assert!(
            warnings[0].message.contains("orphaned"),
            "{:?}",
            warnings[0].message
        );
        assert!(
            !warnings[0].message.contains("broken"),
            "must not use C4's wording — the claim itself still stands: {:?}",
            warnings[0].message
        );
        assert!(only(&report, CheckId::C4).is_empty());
        assert!(
            report.passed(),
            "a Warn-only report must still pass: {:#?}",
            report.diagnostics
        );
    }

    #[test]
    fn a_bare_reference_to_a_nonexistent_target_produces_no_failure() {
        // Criterion 4, the noise-suppression property (R1's core reason
        // for the third kind): a prose link that is declared as neither
        // `depends` nor `because` is bare — it asserts no dependence, so
        // its target not resolving is not this tool's concern at all. No
        // C4, no orphaned-because, no C5 (the new C5 only requires
        // declared refs to have a prose link, never the reverse).
        let dir = tempdir();
        dir.write(
            "docket.ncl",
            r#"{ genres = [ { path = "docs/specs/**", kinds = ["constraint"], quadrant = "reference" } ] }"#,
        );
        dir.write(
            "docs/specs/x.md",
            "### [x]\n\nSee also [related, unrelated work](does-not-exist), mentioned in passing.\n\n```claim\nkind: constraint\nevaluator: test\n```\n",
        );
        let report = run(&dir);
        assert!(
            report.diagnostics.is_empty(),
            "a bare reference must produce no diagnostic at all: {:#?}",
            report.diagnostics
        );
    }

    #[test]
    fn c4_resolves_a_numbered_document_anchor() {
        let dir = tempdir();
        dir.write(
            "docket.ncl",
            r#"{
              genres = [
                { path = "docs/specs/**", kinds = ["constraint"], quadrant = "reference" },
                { path = "docs/models/**", kinds = ["invariant"], quadrant = "reference" },
              ],
            }"#,
        );
        dir.write(
            "docs/models/composition-model.md",
            "## 6. The fact-set: the substrate's only state\n\nprose\n",
        );
        dir.write(
            "docs/specs/x.md",
            "### [x]\n\n```claim\nkind: constraint\nevaluator: test\ndepends: [docs/models/composition-model#6]\n```\n",
        );
        let report = run(&dir);
        assert!(
            only(&report, CheckId::C4).is_empty(),
            "{:#?}",
            report.diagnostics
        );
    }

    #[test]
    fn c4_rejects_a_numeric_prefix_collision() {
        let dir = tempdir();
        dir.write(
            "docket.ncl",
            r#"{
              genres = [
                { path = "docs/specs/**", kinds = ["constraint"], quadrant = "reference" },
                { path = "docs/models/**", kinds = ["invariant"], quadrant = "reference" },
              ],
            }"#,
        );
        dir.write("docs/models/m.md", "## 60. Something else\n");
        dir.write(
            "docs/specs/x.md",
            "### [x]\n\n```claim\nkind: constraint\nevaluator: test\ndepends: [docs/models/m#6]\n```\n",
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
                { path = "docs/specs/**", kinds = ["constraint"], quadrant = "reference" },
                { path = "docs/models/**", kinds = ["invariant"], quadrant = "reference" },
              ],
            }"#,
        );
        dir.write("docs/models/lean/README.md", "## 1. Lean notes\n");
        dir.write("docs/models/tla/README.md", "## 1. TLA notes\n");
        dir.write(
            "docs/specs/x.md",
            "### [x]\n\n```claim\nkind: constraint\nevaluator: test\ndepends: [docs/models/lean/README#1, docs/models/tla/README#1]\n```\n",
        );
        let report = run(&dir);
        assert!(
            only(&report, CheckId::C4).is_empty(),
            "{:#?}",
            report.diagnostics
        );
    }

    #[test]
    fn c5_passes_when_prose_link_covers_a_depends_entry() {
        let dir = tempdir();
        dir.write(
            "docket.ncl",
            r#"{
              genres = [
                { path = "docs/specs/**", kinds = ["constraint"], quadrant = "reference" },
                { path = "docs/models/**", kinds = ["invariant"], quadrant = "reference" },
              ],
            }"#,
        );
        dir.write("docs/models/composition-model.md", "## 6. The fact-set\n");
        dir.write(
            "docs/specs/x.md",
            "### [x]\n\nSee [the fact-set](../models/composition-model.md#6).\n\n```claim\nkind: constraint\nevaluator: test\ndepends: [docs/models/composition-model#6]\n```\n",
        );
        let report = run(&dir);
        assert!(
            only(&report, CheckId::C5).is_empty(),
            "{:#?}",
            report.diagnostics
        );
    }

    #[test]
    fn c5_passes_when_prose_link_covers_a_because_entry() {
        // R3's rule reads "every `depends` AND `because` entry carries a
        // prose link" — this proves C5 actually enforces the `because`
        // half too, not only `depends`.
        let dir = tempdir();
        dir.write(
            "docket.ncl",
            r#"{
              genres = [
                { path = "docs/specs/**", kinds = ["constraint"], quadrant = "reference" },
                { path = "docs/models/**", kinds = ["invariant"], quadrant = "reference" },
              ],
            }"#,
        );
        dir.write("docs/models/composition-model.md", "## 6. The fact-set\n");
        dir.write(
            "docs/specs/x.md",
            "### [x]\n\nSee [the fact-set](../models/composition-model.md#6).\n\n```claim\nkind: constraint\nevaluator: test\nbecause: [docs/models/composition-model#6]\n```\n",
        );
        let report = run(&dir);
        assert!(
            only(&report, CheckId::C5).is_empty(),
            "{:#?}",
            report.diagnostics
        );
    }

    #[test]
    fn c5_fails_when_a_depends_entry_has_no_prose_link() {
        // Criterion 5, first half: a `depends` entry with no matching
        // prose link fails C5's replacement rule.
        let dir = tempdir();
        dir.write(
            "docket.ncl",
            r#"{
              genres = [
                { path = "docs/specs/**", kinds = ["constraint"], quadrant = "reference" },
                { path = "docs/models/**", kinds = ["invariant"], quadrant = "reference" },
              ],
            }"#,
        );
        dir.write("docs/models/composition-model.md", "## 6. The fact-set\n");
        dir.write(
            "docs/specs/x.md",
            "### [x]\n\nNo links here.\n\n```claim\nkind: constraint\nevaluator: test\ndepends: [docs/models/composition-model#6]\n```\n",
        );
        let report = run(&dir);
        let failures = only(&report, CheckId::C5);
        assert_eq!(failures.len(), 1);
        assert!(
            failures[0].message.contains("depends:"),
            "{:?}",
            failures[0].message
        );
    }

    #[test]
    fn c5_fails_when_a_because_entry_has_no_prose_link() {
        let dir = tempdir();
        dir.write(
            "docket.ncl",
            r#"{
              genres = [
                { path = "docs/specs/**", kinds = ["constraint"], quadrant = "reference" },
                { path = "docs/models/**", kinds = ["invariant"], quadrant = "reference" },
              ],
            }"#,
        );
        dir.write("docs/models/composition-model.md", "## 6. The fact-set\n");
        dir.write(
            "docs/specs/x.md",
            "### [x]\n\nNo links here.\n\n```claim\nkind: constraint\nevaluator: test\nbecause: [docs/models/composition-model#6]\n```\n",
        );
        let report = run(&dir);
        let failures = only(&report, CheckId::C5);
        assert_eq!(failures.len(), 1);
        assert!(
            failures[0].message.contains("because:"),
            "{:?}",
            failures[0].message
        );
    }

    #[test]
    fn c5_does_not_require_an_undeclared_prose_link_to_be_declared() {
        // Criterion 5, second half: a prose link with no declaration does
        // NOT fail C5 — it is a bare reference (R3), and C5's new rule is
        // one-directional (declared ⊆ prose), not the old set equality.
        let dir = tempdir();
        dir.write(
            "docket.ncl",
            r#"{ genres = [ { path = "docs/specs/**", kinds = ["constraint"], quadrant = "reference" } ] }"#,
        );
        dir.write(
            "docs/specs/x.md",
            "### [x]\n\nSee [context, not a dependency](https://example.com/context).\n\n```claim\nkind: constraint\nevaluator: test\n```\n",
        );
        let report = run(&dir);
        assert!(
            only(&report, CheckId::C5).is_empty(),
            "{:#?}",
            report.diagnostics
        );
    }

    #[test]
    fn c5_ignores_an_external_url_and_a_link_outside_the_corpus() {
        let dir = tempdir();
        dir.write(
            "docket.ncl",
            r#"{ genres = [ { path = "docs/specs/**", kinds = ["constraint"], quadrant = "reference" } ] }"#,
        );
        dir.write(
            "docs/specs/x.md",
            "### [x]\n\nSee [the web](https://example.com) and [escaping the root](../../outside.md).\n\n```claim\nkind: constraint\nevaluator: test\n```\n",
        );
        let report = run(&dir);
        assert!(
            only(&report, CheckId::C5).is_empty(),
            "{:#?}",
            report.diagnostics
        );
    }

    #[test]
    fn c5_accepts_an_anchor_form_prose_link_for_a_claim_id_declaration() {
        // .ledger/2026-07-30-claim-id-prose-link-breaks-link-checkers.md:
        // the anchor form (`#id`) is what an ordinary link checker and an
        // author both understand for a same-file link. A claim-id
        // `depends` entry must be satisfiable by it, not only by the bare
        // form (`[…](target-claim)`, unlinkable outside this tool).
        let dir = tempdir();
        dir.write(
            "docket.ncl",
            r#"{ genres = [ { path = "docs/specs/**", kinds = ["constraint"], quadrant = "reference" } ] }"#,
        );
        dir.write(
            "docs/specs/a.md",
            "### [target-claim]\n\n```claim\nkind: constraint\nevaluator: test\n```\n\n\
             ### [depends-on-target]\n\nSee [the target](#target-claim).\n\n\
             ```claim\nkind: constraint\nevaluator: test\ndepends: [target-claim]\n```\n",
        );
        let report = run(&dir);
        assert!(
            only(&report, CheckId::C5).is_empty(),
            "{:#?}",
            report.diagnostics
        );
    }

    #[test]
    fn c5_accepts_a_cross_file_anchor_form_prose_link_for_a_claim_id_declaration() {
        // The anchor doesn't have to be same-file: a doc-anchor prose
        // link whose *anchor component* is the claim id satisfies the
        // declaration regardless of which document it points at.
        let dir = tempdir();
        dir.write(
            "docket.ncl",
            r#"{
              genres = [
                { path = "docs/specs/**", kinds = ["constraint"], quadrant = "reference" },
                { path = "docs/models/**", kinds = ["invariant"], quadrant = "reference" },
              ],
            }"#,
        );
        dir.write(
            "docs/specs/a.md",
            "### [target-claim]\n\n```claim\nkind: constraint\nevaluator: test\n```\n",
        );
        dir.write(
            "docs/models/b.md",
            "### [depends-on-target]\n\nSee [the target](../specs/a.md#target-claim).\n\n\
             ```claim\nkind: invariant\nevaluator: test\ndepends: [target-claim]\n```\n",
        );
        let report = run(&dir);
        assert!(
            only(&report, CheckId::C5).is_empty(),
            "{:#?}",
            report.diagnostics
        );
    }

    #[test]
    fn c5_still_fails_a_claim_id_depends_entry_with_no_prose_link_at_all() {
        // The property most easily lost by this fix: accepting the
        // anchor form must not turn into accepting *no* link. A
        // claim-id `depends` entry with zero prose links anywhere in the
        // claim's body still fails C5.
        let dir = tempdir();
        dir.write(
            "docket.ncl",
            r#"{ genres = [ { path = "docs/specs/**", kinds = ["constraint"], quadrant = "reference" } ] }"#,
        );
        dir.write(
            "docs/specs/a.md",
            "### [target-claim]\n\n```claim\nkind: constraint\nevaluator: test\n```\n\n\
             ### [depends-on-target]\n\nNo links here.\n\n\
             ```claim\nkind: constraint\nevaluator: test\ndepends: [target-claim]\n```\n",
        );
        let report = run(&dir);
        let failures = only(&report, CheckId::C5);
        assert_eq!(failures.len(), 1, "{:#?}", report.diagnostics);
        assert!(
            failures[0].message.contains("depends:target-claim"),
            "{:?}",
            failures[0].message
        );
    }

    // --- normative-prose ---------------------------------------------

    #[test]
    fn normative_prose_fails_on_a_bare_keyword_in_a_kinds_empty_genre() {
        // Criterion 2: a `kinds = []` genre holding a bare MUST in its
        // own voice, no claim block anywhere in the file.
        let dir = tempdir();
        dir.write(
            "docket.ncl",
            r#"{ genres = [ { path = "docs/adr/**", kinds = [], quadrant = "explanation" } ] }"#,
        );
        dir.write(
            "docs/adr/0001-decision.md",
            "# ADR 0001: decision\n\nThis decision MUST be treated as final.\n",
        );
        let report = run(&dir);
        assert_eq!(
            only(&report, CheckId::NormativeProse).len(),
            1,
            "{:#?}",
            report.diagnostics
        );
    }

    #[test]
    fn normative_prose_passes_when_the_keyword_is_only_quoted_or_coded() {
        // Criterion 3: the same keyword, present only inside a block
        // quote and inside an inline code span — the check must PASS,
        // proving the design (a quote/code exemption) rather than a grep.
        let dir = tempdir();
        dir.write(
            "docket.ncl",
            r#"{ genres = [ { path = "docs/adr/**", kinds = [], quadrant = "explanation" } ] }"#,
        );
        dir.write(
            "docs/adr/0001-decision.md",
            "# ADR 0001: decision\n\n> The rejected proposal said the service MUST retry.\n\nWe reject that. Note `MUST` above is quoted, not asserted.\n",
        );
        let report = run(&dir);
        assert!(
            only(&report, CheckId::NormativeProse).is_empty(),
            "{:#?}",
            report.diagnostics
        );
    }

    #[test]
    fn normative_prose_is_silent_in_a_genre_that_permits_kinds() {
        // Criterion 4: a genre that permits kinds is unaffected — its job
        // is to say MUST.
        let dir = tempdir();
        dir.write(
            "docket.ncl",
            r#"{ genres = [ { path = "docs/specs/**", kinds = ["constraint"], quadrant = "reference" } ] }"#,
        );
        dir.write(
            "docs/specs/lock.md",
            "### [lock-groundness]\n\nEvery lock value MUST be ground.\n\n```claim\nkind: constraint\nevaluator: test\n```\n",
        );
        let report = run(&dir);
        assert!(
            only(&report, CheckId::NormativeProse).is_empty(),
            "{:#?}",
            report.diagnostics
        );
    }

    #[test]
    fn normative_prose_names_the_file_line_and_genre() {
        let dir = tempdir();
        dir.write(
            "docket.ncl",
            r#"{ genres = [ { path = "docs/adr/**", kinds = [], quadrant = "explanation" } ] }"#,
        );
        dir.write(
            "docs/adr/x.md",
            "# ADR\n\nline2\n\nThis SHALL NOT be reopened.\n",
        );
        let report = run(&dir);
        let failures = only(&report, CheckId::NormativeProse);
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].file, "docs/adr/x.md");
        assert_eq!(failures[0].line, 5);
        assert!(failures[0].message.contains("SHALL NOT"));
        assert!(failures[0].message.contains("docs/adr/**"));
    }

    #[test]
    fn orphan_claim_is_reported() {
        let dir = tempdir();
        dir.write(
            "docket.ncl",
            r#"{ genres = [ { path = "docs/specs/**", kinds = ["constraint"], quadrant = "reference" } ] }"#,
        );
        dir.write(
            "docs/specs/x.md",
            "## Notes\n\n```claim\nkind: constraint\n```\n",
        );
        let report = run(&dir);
        assert_eq!(only(&report, CheckId::OrphanClaim).len(), 1);
    }

    // --- bold-form recognizer: C2 across recognizers, coverage count ------

    #[test]
    fn c2_fails_on_one_id_declared_via_both_heading_and_bold_form() {
        // Criterion 5: two definitions of one id, arriving via DIFFERENT
        // recognizers (heading-form in one file, bold-form in another),
        // must still be caught as a single C2 duplicate naming both
        // sites — C2 groups by `Claim::id` alone, so this needs no new
        // logic, only proof it actually holds.
        let dir = tempdir();
        dir.write(
            "docket.ncl",
            r#"{ genres = [ { path = "docs/specs/**", kinds = ["constraint"], quadrant = "reference" } ] }"#,
        );
        dir.write(
            "docs/specs/a.md",
            "### [dup-across-forms]\n\n```claim\nkind: constraint\nevaluator: test\n```\n",
        );
        dir.write(
            "docs/specs/b.md",
            "**[dup-across-forms]**: A second definition, bold-form this time.\n\n```claim\nkind: constraint\nevaluator: test\n```\n",
        );
        let report = run(&dir);
        let failures = only(&report, CheckId::C2);
        // One diagnostic per site (existing C2 shape), each naming the
        // OTHER site in its message — together the pair names both
        // locations, exactly what the dispatch's criterion 5 asks for.
        assert_eq!(failures.len(), 2, "{:#?}", report.diagnostics);
        let at_a = failures.iter().find(|d| d.file == "docs/specs/a.md");
        let at_b = failures.iter().find(|d| d.file == "docs/specs/b.md");
        assert!(at_a.is_some() && at_b.is_some(), "{:#?}", failures);
        assert!(at_a.unwrap().message.contains("docs/specs/b.md"));
        assert!(at_b.unwrap().message.contains("docs/specs/a.md"));
    }

    #[test]
    fn unregistered_definition_is_reported_but_never_fails_the_report() {
        let dir = tempdir();
        dir.write(
            "docket.ncl",
            r#"{ genres = [ { path = "docs/specs/**", kinds = ["constraint"], quadrant = "reference" } ] }"#,
        );
        dir.write(
            "docs/specs/x.md",
            "**[not-yet-registered]**: A real definition with no block yet.\n",
        );
        let report = run(&dir);
        let warnings = only(&report, CheckId::UnregisteredDefinition);
        assert_eq!(warnings.len(), 1, "{:#?}", report.diagnostics);
        assert_eq!(warnings[0].severity, Severity::Warn);
        assert!(warnings[0].message.contains("not-yet-registered"));
        assert!(
            report.passed(),
            "unregistered-definition must never flip the exit code: {:#?}",
            report.diagnostics
        );
    }

    #[test]
    fn a_registered_bold_form_claim_passes_every_check_cleanly() {
        let dir = tempdir();
        dir.write(
            "docket.ncl",
            r#"{
              genres = [
                { path = "docs/specs/**", kinds = ["constraint"], quadrant = "reference" },
                { path = "docs/models/**", kinds = ["invariant"], quadrant = "reference" },
              ],
            }"#,
        );
        dir.write("docs/models/composition-model.md", "## 6. The fact-set\n");
        dir.write(
            "docs/specs/x.md",
            "**[bold-registered]**: A bold-form claim that depends on\na model section, declared via a real relative markdown link.\n\nSee [the fact-set](../models/composition-model.md#6).\n\n```claim\nkind: constraint\nevaluator: test\ndepends: [docs/models/composition-model#6]\n```\n",
        );
        let report = run(&dir);
        assert!(report.passed(), "{:#?}", report.diagnostics);
        assert!(
            only(&report, CheckId::UnregisteredDefinition).is_empty(),
            "{:#?}",
            report.diagnostics
        );
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
