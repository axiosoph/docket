//! Blast radius (MVP.md §4.2): given a ref — a claim id or a
//! `<doc-path>#<anchor>` document anchor (§1.3) — every claim (and the
//! document it lives in) that transitively cites it — "the set a
//! reviewer must re-check if it changes." Computed purely over the
//! citation graph (README.md, "Links are data": "never by
//! pattern-matching prose").
//!
//! **Both `depends` and `because` create edges here, undistinguished.**
//! R2 (`.ledger/2026-07-30-reference-kinds-and-document-resolution.md`)
//! separates them into two different *searches* — forward for what
//! consumes a target, backward for what is justified by it — but blast
//! radius answers a third question, "what must a reviewer re-check", and
//! both kinds answer it identically: a claim that depends on a changed
//! target may now be wrong, and a claim justified by it may now need a
//! new reason. A **bare** reference never enters this graph at all — it
//! is not a value [`crate::model::Claim::refs`] ever yields.
//!
//! Every ref form creates a reverse edge, keyed by the cited ref's
//! canonical string (`CiteRef::to_string()` — a bare id for a claim, or
//! `path#anchor` for a document anchor). A document anchor is never
//! itself a *citer* (only a claim has `depends`/`because`), so once a
//! claim citing a document anchor is found, the walk continues from that
//! claim's own id exactly as it would from any other claim-id node —
//! the recursion is agnostic to which ref form found it, and to which
//! kind carried it.

use crate::model::{Claim, Corpus};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlastEntry {
    pub claim: String,
    pub file: String,
}

/// A detected cycle: `claim` cites back to `ancestor`, which is already
/// on the current traversal path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cycle {
    pub claim: String,
    pub ancestor: String,
}

#[derive(Debug, Clone, Default)]
pub struct BlastResult {
    /// Every citing claim, in discovery order, deduplicated.
    pub entries: Vec<BlastEntry>,
    pub cycles: Vec<Cycle>,
}

/// The reverse-citation map every graph-derived query walks: keyed by the
/// cited ref's canonical string, so a claim-id cite and a document-anchor
/// cite both register a reverse edge under the same vocabulary a raw ref
/// argument is written in. The two key spaces never collide: a claim id is
/// bare kebab-case, a document anchor's string always contains `#`
/// (`CiteRef::Display`).
///
/// `pub(crate)` rather than private: `signals.rs` needs the same reverse
/// edges for in-degree and reuses this rather than re-deriving its own
/// copy of how an edge is keyed — the exact duplication R6
/// (`.ledger/2026-07-30-reference-kinds-and-document-resolution.md`) warns
/// against, applied to this crate's own internals.
pub(crate) fn citers_map(corpus: &Corpus) -> HashMap<String, Vec<&Claim>> {
    let mut citers: HashMap<String, Vec<&Claim>> = HashMap::new();
    for claim in &corpus.claims {
        for (_kind, cite) in claim.refs() {
            citers.entry(cite.to_string()).or_default().push(claim);
        }
    }
    citers
}

pub fn blast_radius(corpus: &Corpus, start: &str) -> BlastResult {
    let citers = citers_map(corpus);

    let mut visited: HashSet<&str> = HashSet::new();
    let mut stack: Vec<&str> = vec![start];
    let mut result = BlastResult::default();
    visited.insert(start);

    visit(start, &citers, &mut visited, &mut stack, &mut result);

    result
}

fn visit<'a>(
    node: &str,
    citers: &HashMap<String, Vec<&'a Claim>>,
    visited: &mut HashSet<&'a str>,
    stack: &mut Vec<&'a str>,
    result: &mut BlastResult,
) {
    let Some(citing_claims) = citers.get(node) else {
        return;
    };
    for claim in citing_claims {
        let id = claim.id.as_str();
        if stack.contains(&id) {
            // A back-edge to an ancestor on the current path: a real
            // cycle. Report it; don't follow it again.
            result.cycles.push(Cycle {
                claim: id.to_string(),
                ancestor: node.to_string(),
            });
            continue;
        }
        if visited.insert(id) {
            result.entries.push(BlastEntry {
                claim: id.to_string(),
                file: claim.file.clone(),
            });
            stack.push(id);
            visit(id, citers, visited, stack, result);
            stack.pop();
        }
        // Otherwise already fully explored via another path (a diamond
        // reconvergence, not a cycle) — nothing to add or report.
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extract::extract_document;

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

    /// Every existing blast test only needs to prove the graph walk, not
    /// which kind carries each edge — `depends` is the arbitrary but
    /// representative choice; `finds_citers_via_because_edges_too` below
    /// is what actually proves `because` edges are walked identically.
    fn claim_src(id: &str, depends: &[&str]) -> String {
        claim_src_kind(id, "depends", depends)
    }

    fn corpus_with_claims(claims: Vec<Claim>) -> Corpus {
        Corpus {
            claims,
            ..Default::default()
        }
    }

    #[test]
    fn finds_direct_and_transitive_citers() {
        // a <- b <- c (c cites b, b cites a): blasting `a` should surface
        // both b and c.
        let a = claim_src("a", &[]);
        let b = claim_src("b", &["a"]);
        let c = claim_src("c", &["b"]);
        let corpus = corpus_with_claims(claims_from(&[
            ("f-a.md", &a),
            ("f-b.md", &b),
            ("f-c.md", &c),
        ]));

        let result = blast_radius(&corpus, "a");
        let ids: Vec<&str> = result.entries.iter().map(|e| e.claim.as_str()).collect();
        assert_eq!(ids, vec!["b", "c"]);
        assert!(result.cycles.is_empty());
    }

    #[test]
    fn a_diamond_is_not_a_cycle() {
        // a <- b, a <- c, b <- d, c <- d (d cites both b and c; b and c
        // both cite a). d must appear exactly once.
        let a = claim_src("a", &[]);
        let b = claim_src("b", &["a"]);
        let c = claim_src("c", &["a"]);
        let d = claim_src("d", &["b", "c"]);
        let corpus = corpus_with_claims(claims_from(&[
            ("f-a.md", &a),
            ("f-b.md", &b),
            ("f-c.md", &c),
            ("f-d.md", &d),
        ]));

        let result = blast_radius(&corpus, "a");
        let ids: Vec<&str> = result.entries.iter().map(|e| e.claim.as_str()).collect();
        assert_eq!(ids.iter().filter(|id| **id == "d").count(), 1);
        assert!(
            result.cycles.is_empty(),
            "a diamond is not a cycle: {:?}",
            result.cycles
        );
    }

    #[test]
    fn a_real_cycle_is_reported_and_not_followed_forever() {
        // a <- b <- c <- b (c cites b, closing a cycle back to b).
        let a = claim_src("a", &[]);
        let b = claim_src("b", &["a", "c"]);
        let c = claim_src("c", &["b"]);
        let corpus = corpus_with_claims(claims_from(&[
            ("f-a.md", &a),
            ("f-b.md", &b),
            ("f-c.md", &c),
        ]));

        let result = blast_radius(&corpus, "a");
        let ids: Vec<&str> = result.entries.iter().map(|e| e.claim.as_str()).collect();
        assert_eq!(ids, vec!["b", "c"]);
        assert_eq!(
            result.cycles,
            vec![Cycle {
                claim: "b".to_string(),
                ancestor: "c".to_string()
            }]
        );
    }

    #[test]
    fn an_unknown_start_id_yields_an_empty_result() {
        let corpus = corpus_with_claims(claims_from(&[("f.md", &claim_src("a", &[]))]));
        let result = blast_radius(&corpus, "does-not-exist");
        assert!(result.entries.is_empty());
        assert!(result.cycles.is_empty());
    }

    #[test]
    fn a_document_anchor_cite_creates_a_reverse_edge() {
        // A claim citing a document anchor (not a claim id) must surface
        // when blasting that exact `path#anchor` ref — the defect this
        // change fixes: MVP.md's motivating example (§4.1's worked-example
        // dependency) is exactly this shape.
        let src =
            "### [x]\n\n```claim\nkind: constraint\nevaluator: test\ndepends: [some-doc#3]\n```\n";
        let corpus = corpus_with_claims(extract_document("f.md", src).claims);

        let result = blast_radius(&corpus, "some-doc#3");
        let ids: Vec<&str> = result.entries.iter().map(|e| e.claim.as_str()).collect();
        assert_eq!(ids, vec!["x"]);
    }

    #[test]
    fn a_claim_id_argument_still_ignores_an_unrelated_document_anchor() {
        // The bare claim id "some-doc" (no `#`) never matches the
        // document-anchor key "some-doc#3" — the two ref forms occupy
        // disjoint string spaces, so this must stay empty.
        let src =
            "### [x]\n\n```claim\nkind: constraint\nevaluator: test\ndepends: [some-doc#3]\n```\n";
        let corpus = corpus_with_claims(extract_document("f.md", src).claims);
        let result = blast_radius(&corpus, "some-doc");
        assert!(result.entries.is_empty());
    }

    #[test]
    fn the_walk_continues_past_a_document_anchor_citer_via_its_claim_id() {
        // some-doc#3 <- x <- y (y cites x by claim id; x cites the
        // document anchor). Blasting the anchor must reach both: a
        // document-anchor citer is not a graph dead end, since other
        // claims can still cite that citer by its own claim id.
        let x =
            "### [x]\n\n```claim\nkind: constraint\nevaluator: test\ndepends: [some-doc#3]\n```\n";
        let y = claim_src("y", &["x"]);
        let corpus = corpus_with_claims(claims_from(&[("f-x.md", x), ("f-y.md", &y)]));

        let result = blast_radius(&corpus, "some-doc#3");
        let ids: Vec<&str> = result.entries.iter().map(|e| e.claim.as_str()).collect();
        assert_eq!(ids, vec!["x", "y"]);
        assert!(result.cycles.is_empty());
    }

    #[test]
    fn finds_citers_via_because_edges_too() {
        // Blast radius answers "what must a reviewer re-check", which a
        // `because` edge answers exactly as a `depends` edge does — the
        // kind distinction is about severity on deletion (checks.rs), not
        // about whether the edge belongs in this graph at all.
        let a = claim_src("a", &[]);
        let b = claim_src_kind("b", "because", &["a"]);
        let corpus = corpus_with_claims(claims_from(&[("f-a.md", &a), ("f-b.md", &b)]));

        let result = blast_radius(&corpus, "a");
        let ids: Vec<&str> = result.entries.iter().map(|e| e.claim.as_str()).collect();
        assert_eq!(ids, vec!["b"]);
    }
}
