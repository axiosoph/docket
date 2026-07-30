//! Blast radius (MVP.md §4.2): given a claim id, every claim (and the
//! document it lives in) that transitively cites it — "the set a
//! reviewer must re-check if it changes." Computed purely over `cites`
//! (README.md, "Links are data": "never by pattern-matching prose").
//!
//! Only claim-id `cites` entries form reverse edges here — a
//! document-anchor `cites` entry targets a *heading*, not a claim, so it
//! can never itself be the claim whose citers we're walking.

use crate::model::{Claim, CiteRef, Corpus};
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

pub fn blast_radius(corpus: &Corpus, start: &str) -> BlastResult {
    let mut citers: HashMap<&str, Vec<&Claim>> = HashMap::new();
    for claim in &corpus.claims {
        for cite in &claim.cites {
            if let CiteRef::Claim(id) = cite {
                citers.entry(id.as_str()).or_default().push(claim);
            }
        }
    }

    let mut visited: HashSet<&str> = HashSet::new();
    let mut stack: Vec<&str> = vec![start];
    let mut result = BlastResult::default();
    visited.insert(start);

    visit(start, &citers, &mut visited, &mut stack, &mut result);

    result
}

fn visit<'a>(
    node: &str,
    citers: &HashMap<&'a str, Vec<&'a Claim>>,
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

    fn claim_src(id: &str, cites: &[&str]) -> String {
        format!(
            "### [{id}]\n\n```claim\nkind: constraint\nevaluator: test\ncites: [{}]\n```\n",
            cites.join(", ")
        )
    }

    #[test]
    fn finds_direct_and_transitive_citers() {
        // a <- b <- c (c cites b, b cites a): blasting `a` should surface
        // both b and c.
        let a = claim_src("a", &[]);
        let b = claim_src("b", &["a"]);
        let c = claim_src("c", &["b"]);
        let mut corpus = Corpus::default();
        corpus.claims = claims_from(&[("f-a.md", &a), ("f-b.md", &b), ("f-c.md", &c)]);

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
        let mut corpus = Corpus::default();
        corpus.claims = claims_from(&[("f-a.md", &a), ("f-b.md", &b), ("f-c.md", &c), ("f-d.md", &d)]);

        let result = blast_radius(&corpus, "a");
        let ids: Vec<&str> = result.entries.iter().map(|e| e.claim.as_str()).collect();
        assert_eq!(ids.iter().filter(|id| **id == "d").count(), 1);
        assert!(result.cycles.is_empty(), "a diamond is not a cycle: {:?}", result.cycles);
    }

    #[test]
    fn a_real_cycle_is_reported_and_not_followed_forever() {
        // a <- b <- c <- b (c cites b, closing a cycle back to b).
        let a = claim_src("a", &[]);
        let b = claim_src("b", &["a", "c"]);
        let c = claim_src("c", &["b"]);
        let mut corpus = Corpus::default();
        corpus.claims = claims_from(&[("f-a.md", &a), ("f-b.md", &b), ("f-c.md", &c)]);

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
        let mut corpus = Corpus::default();
        corpus.claims = claims_from(&[("f.md", &claim_src("a", &[]))]);
        let result = blast_radius(&corpus, "does-not-exist");
        assert!(result.entries.is_empty());
        assert!(result.cycles.is_empty());
    }

    #[test]
    fn document_anchor_cites_do_not_create_reverse_edges() {
        // A claim that only cites a document anchor never counts as a
        // citer of any claim id.
        let src = "### [x]\n\n```claim\nkind: constraint\nevaluator: test\ncites: [some-doc#3]\n```\n";
        let mut corpus = Corpus::default();
        corpus.claims = extract_document("f.md", src).claims;
        let result = blast_radius(&corpus, "some-doc");
        assert!(result.entries.is_empty());
    }
}
