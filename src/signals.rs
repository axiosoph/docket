//! Derived properties the register already carries — no new declaration,
//! nothing an author writes (MVP.md §R4:
//! `.ledger/2026-07-30-reference-kinds-and-document-resolution.md`,
//! "anything derivable from the reference graph is computed, never
//! declared"). Most signals here are a pure read of `Claim::refs()` and
//! `blast::blast_radius`, over the same graph `blast` already walks — no
//! new traversal; [`legacy_target_ambiguous`] is a pure read of a single
//! claim's own `evaluator` field instead, a different axis of the same
//! "candidate for a reader to weigh, not a verdict" framing.
//!
//! The headline graph signal is in-degree zero, the backward read
//! `.ledger/2026-08-05-links-are-document-facts-not-claim-attributes.md`
//! ("the head's extension") names directly: **a claim nothing points at is
//! a candidate for superseded-and-unnoticed** — nothing depends on it and
//! nothing justifies itself by it, the mechanical form of the project's
//! dominant failure mode. It is a **candidate, not a verdict**: a
//! self-contained leaf claim (a forbidden state, a standalone safety
//! property) can be perfectly current with zero inbound edges by design.
//! This module bounds the search; the CLI (`main.rs::print_signals`)
//! carries the framing that keeps a reader from reading the list as an
//! accusation.

use crate::blast::{blast_radius, citers_map};
use crate::model::Corpus;

/// One claim's position in the reference graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimSignals {
    pub id: String,
    pub file: String,
    pub line: usize,
    /// `claim.refs().count()` — how many `depends`/`because` entries this
    /// claim declares, to a claim id or a document anchor.
    pub out_degree: usize,
    /// How many `depends`/`because` entries anywhere in the corpus name
    /// this claim's bare id directly. A document-anchor edge never counts
    /// here — the two key spaces are disjoint by construction
    /// (`model::CiteRef`), so an anchor citing this claim's file can never
    /// be mistaken for a citer of the claim itself.
    pub in_degree: usize,
    /// `blast::blast_radius(corpus, &id).entries.len()` — every claim
    /// that transitively depends on or is justified by this one. This is
    /// what removing the claim costs operationally: its direct citers
    /// break outright (C4 / `orphaned-because`), and the rest are exactly
    /// blast.rs's own "set a reviewer must re-check if it changes" —
    /// reused, not a new computation, and not itself a claim that every
    /// member of that set becomes literally unreachable.
    pub review_surface: usize,
}

/// The full corpus's derived graph signals, one entry per claim.
pub struct GraphSignals {
    pub claims: Vec<ClaimSignals>,
}

impl GraphSignals {
    /// Zero-inbound claims, in corpus order — the headline signal. A
    /// **candidate** list: every entry here bounds where a reviewer might
    /// look, and none of them is a finding on its own.
    pub fn zero_inbound(&self) -> impl Iterator<Item = &ClaimSignals> {
        self.claims.iter().filter(|c| c.in_degree == 0)
    }
}

/// Compute every claim's degree and review-surface over the corpus's
/// existing reference graph. `O(claims × edges)`: one `blast_radius` call
/// per claim, each no more expensive than `docket blast` already is for a
/// single ref — cheap at corpus scale (hundreds of claims, not millions).
pub fn compute(corpus: &Corpus) -> GraphSignals {
    let citers = citers_map(corpus);
    let claims = corpus
        .claims
        .iter()
        .map(|claim| {
            let in_degree = citers.get(claim.id.as_str()).map_or(0, Vec::len);
            let out_degree = claim.refs().count();
            let review_surface = blast_radius(corpus, &claim.id).entries.len();
            ClaimSignals {
                id: claim.id.clone(),
                file: claim.file.clone(),
                line: claim.block_line.0,
                out_degree,
                in_degree,
                review_surface,
            }
        })
        .collect();
    GraphSignals { claims }
}

/// A claim whose legacy `evaluator` grade cannot be mechanically resolved
/// to a verification target — `review` and `proof` straddle the
/// design/implementation axis (`contracts/register.ncl`'s
/// `legacy_target`: every other legacy value maps to exactly one axis,
/// these two don't), so only the claim's own prose can say which. This is
/// not a defect: the claim is not wrong, merely unmigrated to the
/// two-key `verification:` shape (`contracts/claim.ncl`) that resolves
/// the ambiguity by construction. A candidate for a reader to weigh when
/// migrating a claim, never a `check` finding — deliberately mirrored
/// here in Rust rather than read back from the register's index: this
/// needs only `Claim::raw.evaluator`, already in hand, and a claim
/// written in the new shape can never trigger it (a `review`/`proof`
/// placed explicitly under `design`/`implementation` is resolved by the
/// author, not ambiguous).
pub struct LegacyTargetAmbiguous {
    pub id: String,
    pub file: String,
    pub line: usize,
    /// `"review"` or `"proof"` — the only two legacy values this fires
    /// for.
    pub evaluator: String,
}

/// Every claim in the corpus still carrying a target-ambiguous legacy
/// `evaluator` value, in corpus order.
pub fn legacy_target_ambiguous(corpus: &Corpus) -> Vec<LegacyTargetAmbiguous> {
    corpus
        .claims
        .iter()
        .filter_map(|c| {
            let evaluator = c.raw.evaluator.as_deref()?;
            if evaluator == "review" || evaluator == "proof" {
                Some(LegacyTargetAmbiguous {
                    id: c.id.clone(),
                    file: c.file.clone(),
                    line: c.block_line.0,
                    evaluator: evaluator.to_string(),
                })
            } else {
                None
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extract::extract_document;
    use crate::model::Claim;

    fn claims_from(sources: &[(&str, &str)]) -> Vec<Claim> {
        sources
            .iter()
            .flat_map(|(file, src)| extract_document(file, src).claims)
            .collect()
    }

    fn claim_src_kind(id: &str, field: &str, refs: &[&str]) -> String {
        format!(
            "### [{id}]\n\n```claim\nkind: constraint\nevaluator: test\n{field}: [{}]\n```\n",
            refs.join(", ")
        )
    }

    fn claim_src(id: &str, depends: &[&str]) -> String {
        claim_src_kind(id, "depends", depends)
    }

    fn corpus_with_claims(claims: Vec<Claim>) -> Corpus {
        Corpus {
            claims,
            ..Default::default()
        }
    }

    fn find<'a>(signals: &'a GraphSignals, id: &str) -> &'a ClaimSignals {
        signals
            .claims
            .iter()
            .find(|c| c.id == id)
            .unwrap_or_else(|| panic!("no signals computed for claim {id:?}"))
    }

    #[test]
    fn a_claim_nothing_cites_is_zero_inbound() {
        let a = claim_src("a", &[]);
        let corpus = corpus_with_claims(claims_from(&[("f-a.md", &a)]));
        let signals = compute(&corpus);
        assert_eq!(find(&signals, "a").in_degree, 0);
        let zero: Vec<&str> = signals.zero_inbound().map(|c| c.id.as_str()).collect();
        assert_eq!(zero, vec!["a"]);
    }

    #[test]
    fn a_claim_with_a_citer_is_not_zero_inbound() {
        let a = claim_src("a", &[]);
        let b = claim_src("b", &["a"]);
        let corpus = corpus_with_claims(claims_from(&[("f-a.md", &a), ("f-b.md", &b)]));
        let signals = compute(&corpus);
        assert_eq!(find(&signals, "a").in_degree, 1);
        assert!(!signals.zero_inbound().any(|c| c.id == "a"));
    }

    #[test]
    fn a_because_edge_counts_toward_in_degree_same_as_depends() {
        // The dispatch's own framing: "nothing depends on it and nothing
        // justifies itself by it" — a `because` citer must clear a claim
        // out of zero-inbound exactly as a `depends` citer does, matching
        // blast.rs's own kind-agnostic treatment.
        let a = claim_src("a", &[]);
        let b = claim_src_kind("b", "because", &["a"]);
        let corpus = corpus_with_claims(claims_from(&[("f-a.md", &a), ("f-b.md", &b)]));
        let signals = compute(&corpus);
        assert_eq!(find(&signals, "a").in_degree, 1);
    }

    #[test]
    fn out_degree_counts_both_depends_and_because() {
        let src = "### [x]\n\n```claim\nkind: constraint\nevaluator: test\n\
                    depends: [a, b]\nbecause: [c]\n```\n";
        let corpus = corpus_with_claims(extract_document("f.md", src).claims);
        assert_eq!(find(&compute(&corpus), "x").out_degree, 3);
    }

    #[test]
    fn a_document_anchor_citer_never_inflates_a_claims_in_degree() {
        // model::CiteRef's disjoint key spaces (a bare id vs. `path#anchor`)
        // must hold here too: a claim named `some-doc` is a different node
        // from the document anchor `some-doc#3`, so a claim citing that
        // anchor must not be counted as a citer of a claim literally named
        // `some-doc`, if one existed.
        let doc_citer =
            "### [x]\n\n```claim\nkind: constraint\nevaluator: test\ndepends: [some-doc#3]\n```\n";
        let named = claim_src("some-doc", &[]);
        let corpus = corpus_with_claims(claims_from(&[
            ("f-x.md", doc_citer),
            ("f-named.md", &named),
        ]));
        let signals = compute(&corpus);
        assert_eq!(find(&signals, "some-doc").in_degree, 0);
    }

    #[test]
    fn review_surface_matches_blast_radius_size() {
        // a <- b <- c: blasting `a` surfaces b and c (2 entries), and
        // that must be exactly `a`'s review_surface — this signal is a
        // reuse of blast.rs's own traversal, not a parallel computation
        // that could drift from it.
        let a = claim_src("a", &[]);
        let b = claim_src("b", &["a"]);
        let c = claim_src("c", &["b"]);
        let corpus = corpus_with_claims(claims_from(&[
            ("f-a.md", &a),
            ("f-b.md", &b),
            ("f-c.md", &c),
        ]));
        let signals = compute(&corpus);
        assert_eq!(find(&signals, "a").review_surface, 2);
        assert_eq!(find(&signals, "b").review_surface, 1);
        assert_eq!(find(&signals, "c").review_surface, 0);
    }

    #[test]
    fn a_zero_inbound_claim_always_has_zero_review_surface() {
        // Tautological, but worth pinning: nothing citing a claim means
        // nothing to walk outward from it either — review_surface and
        // in_degree agree at the boundary rather than silently diverging.
        let a = claim_src("a", &[]);
        let corpus = corpus_with_claims(claims_from(&[("f-a.md", &a)]));
        let signals = compute(&corpus);
        let sig = find(&signals, "a");
        assert_eq!(sig.in_degree, 0);
        assert_eq!(sig.review_surface, 0);
    }

    #[test]
    fn an_empty_corpus_yields_no_signals_and_no_zero_inbound() {
        let corpus = corpus_with_claims(vec![]);
        let signals = compute(&corpus);
        assert!(signals.claims.is_empty());
        assert_eq!(signals.zero_inbound().count(), 0);
    }

    // --- legacy_target_ambiguous ---------------------------------------

    #[test]
    fn legacy_review_and_proof_are_target_ambiguous() {
        let a = "### [a]\n\n```claim\nkind: constraint\nevaluator: review\n```\n";
        let b = "### [b]\n\n```claim\nkind: constraint\nevaluator: proof\n```\n";
        let corpus = corpus_with_claims(claims_from(&[("f-a.md", a), ("f-b.md", b)]));
        let ambiguous = legacy_target_ambiguous(&corpus);
        let ids: Vec<&str> = ambiguous.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, vec!["a", "b"]);
        assert_eq!(ambiguous[0].evaluator, "review");
        assert_eq!(ambiguous[1].evaluator, "proof");
    }

    #[test]
    fn an_ordinary_legacy_evaluator_is_never_target_ambiguous() {
        let a = claim_src("a", &[]);
        let corpus = corpus_with_claims(claims_from(&[("f-a.md", &a)]));
        assert!(legacy_target_ambiguous(&corpus).is_empty());
    }

    #[test]
    fn a_new_shape_claim_naming_review_under_an_axis_is_never_target_ambiguous() {
        // The whole point of the second axis: once an author places
        // `review` explicitly under `design` or `implementation`, the
        // ambiguity this signal exists to flag is already resolved — by
        // construction, not by a rule this function has to apply. This
        // claim has no top-level `evaluator` field at all, so
        // `Claim::raw.evaluator` is `None` and the filter never matches.
        let src = "### [a]\n\n```claim\nkind: constraint\nverification:\n  design: review\n```\n";
        let corpus = corpus_with_claims(claims_from(&[("f-a.md", src)]));
        assert!(legacy_target_ambiguous(&corpus).is_empty());
    }
}
