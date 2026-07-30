//! The index (MVP.md §4.1): a pure projection of the corpus, never
//! committed.

use crate::model::{Corpus, Index, IndexClaim, IndexDocument};

pub fn build_index(corpus: &Corpus) -> Index {
    let mut index = Index::default();

    for claim in &corpus.claims {
        index.claims.insert(
            claim.id.clone(),
            IndexClaim {
                file: claim.file.clone(),
                line: claim.block_line.0,
                kind: claim.raw.kind.clone().unwrap_or_default(),
                evaluator: claim.raw.evaluator.clone().unwrap_or_default(),
                depends: claim.depends.iter().map(|c| c.to_string()).collect(),
                because: claim.because.iter().map(|c| c.to_string()).collect(),
            },
        );
    }

    for doc in &corpus.documents {
        index.documents.insert(
            doc.doc_path.clone(),
            IndexDocument {
                file: doc.file.clone(),
                genre: doc.genre_path.clone(),
            },
        );
    }

    index
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extract::extract_document;
    use crate::model::{Document, Heading, Line};

    #[test]
    fn matches_the_mvp_example_shape() {
        let src = "### [lock-groundness]\n\nEvery lock value MUST be ground: names bound to content identities and exact version strings.\n\n```claim\nkind: constraint\nevaluator: property-test\ndepends: [docs/models/composition-model#6, docs/models/execution-model#2.4]\n```\n";
        let result = extract_document("docs/specs/lock-file-schema.md", src);
        let corpus = Corpus {
            claims: result.claims,
            documents: vec![Document {
                doc_path: "docs/specs/lock-file-schema".to_string(),
                file: "docs/specs/lock-file-schema.md".to_string(),
                genre_path: "docs/specs/**".to_string(),
                headings: vec![Heading {
                    level: 3,
                    text: "[lock-groundness]".to_string(),
                    line: Line(1),
                }],
            }],
        };

        let index = build_index(&corpus);

        let claim = index.claims.get("lock-groundness").expect("claim indexed");
        assert_eq!(claim.file, "docs/specs/lock-file-schema.md");
        assert_eq!(claim.kind, "constraint");
        assert_eq!(claim.evaluator, "property-test");
        assert_eq!(
            claim.depends,
            vec![
                "docs/models/composition-model#6",
                "docs/models/execution-model#2.4"
            ]
        );
        assert!(claim.because.is_empty());

        // The index's document key is the same path-based identifier
        // cites/C4 resolve against (MVP.md §1.3) — not the bare basename
        // MVP.md §4.1's worked JSON example still shows, which would
        // silently reintroduce the basename-collision bug §1.3's own
        // rationale retired duplicate-stem to fix. Flagged to team-lead
        // as a stale example rather than resolved by picking a
        // different, inconsistent identifier scheme just for the index.
        let doc = index
            .documents
            .get("docs/specs/lock-file-schema")
            .expect("document indexed");
        assert_eq!(doc.file, "docs/specs/lock-file-schema.md");
        assert_eq!(doc.genre, "docs/specs/**");
    }

    #[test]
    fn serializes_to_json_with_the_expected_keys() {
        let src = "### [x]\n\n```claim\nkind: invariant\nevaluator: none\n```\n";
        let result = extract_document("docs/models/m.md", src);
        let corpus = Corpus {
            claims: result.claims,
            ..Default::default()
        };

        let index = build_index(&corpus);
        let json = serde_json::to_value(&index).unwrap();
        assert!(json["claims"]["x"]["kind"] == "invariant");
        assert_eq!(json["claims"]["x"]["depends"], serde_json::json!([]));
        assert_eq!(json["claims"]["x"]["because"], serde_json::json!([]));
    }
}
