//! Core data types shared by extraction, checks, the index, and blast radius.
//!
//! These mirror MVP.md §1.2 (claim fields), §1.3 (reference syntax), and
//! §4.1 (index shape) directly — the vocabulary here is the spec's
//! vocabulary, not an invented abstraction over it.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// A claim id: a bracketed kebab-case token, without the brackets.
pub type ClaimId = String;

/// A document stem: a corpus-relative file basename without extension.
pub type DocStem = String;

/// MVP.md §1.2: the three permitted claim kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    Requirement,
    Invariant,
    Constraint,
}

impl Kind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Kind::Requirement => "requirement",
            Kind::Invariant => "invariant",
            Kind::Constraint => "constraint",
        }
    }
}

/// MVP.md §1.2: the six permitted evaluator names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Evaluator {
    Proof,
    ModelCheck,
    PropertyTest,
    Test,
    Example,
    None,
}

/// MVP.md §1.3: a `cites` entry is either a claim id or a document anchor.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum CiteRef {
    /// A bare kebab-case claim id, e.g. `lock-groundness`.
    Claim(ClaimId),
    /// `<doc-stem>#<anchor>`, e.g. `composition-model#6`.
    DocAnchor { stem: DocStem, anchor: String },
}

impl CiteRef {
    /// Parse the `<doc-stem>#<anchor>` / bare-id surface syntax shared by
    /// `cites` entries (MVP.md §1.3) and prose link targets (§3, C5).
    pub fn parse(raw: &str) -> CiteRef {
        match raw.split_once('#') {
            Some((stem, anchor)) => CiteRef::DocAnchor {
                stem: stem.to_string(),
                anchor: anchor.to_string(),
            },
            None => CiteRef::Claim(raw.to_string()),
        }
    }
}

impl std::fmt::Display for CiteRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CiteRef::Claim(id) => write!(f, "{id}"),
            CiteRef::DocAnchor { stem, anchor } => write!(f, "{stem}#{anchor}"),
        }
    }
}

/// A 1-indexed source location within a single file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Line(pub usize);

impl std::fmt::Display for Line {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A raw field value as it appeared in a claim block's YAML, kept around so
/// C1 can report exactly what nickel rejected without re-deriving it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RawClaimBlock {
    /// The block's YAML content, verbatim — this is what gets handed to
    /// the Nickel contract for C1 (MVP.md §7: "invoke Nickel rather than
    /// reimplementing its checking").
    pub yaml: String,
    /// Best-effort structured view, present only where the YAML was at
    /// least parseable; absence here does not imply C1 passed or failed,
    /// it only means this checker didn't reimplement contract validation.
    pub kind: Option<String>,
    pub evaluator: Option<String>,
    pub cites: Vec<String>,
}

/// A claim as extracted from the corpus, before any check has run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Claim {
    pub id: ClaimId,
    pub file: String,
    /// Line of the id heading.
    pub heading_line: Line,
    /// Line of the opening ` ```claim ` fence.
    pub block_line: Line,
    pub raw: RawClaimBlock,
    /// `cites` entries parsed via [`CiteRef::parse`], best-effort.
    pub cites: Vec<CiteRef>,
    /// Raw, un-normalized link targets found in the claim's prose body
    /// (§3, C5's `L`, before restriction to resolving targets). External
    /// URLs are dropped at extraction time since C5 ignores them
    /// unconditionally; everything else is a real markdown href (a
    /// relative path, possibly with a `#fragment`) that checks.rs
    /// resolves against the corpus's documents and claim ids.
    pub prose_links: Vec<String>,
}

/// A heading found anywhere in a scanned document, kept for anchor
/// resolution (`<doc-stem>#<anchor>`, §1.3). `text` is already stripped of
/// `#` markers and leading whitespace — that's how pulldown-cmark hands us
/// heading content — so it's ready for [`anchor_matches`] as-is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Heading {
    pub level: u8,
    pub text: String,
    pub line: Line,
}

/// MVP.md §1.3, "Anchor derivation": an anchor `A` matches a heading iff
/// the heading's text — after stripping `#` markers and leading
/// whitespace — begins with `A` followed by either end-of-string or a
/// non-alphanumeric character. So `6` matches `"6. The fact-set: ..."`
/// but not `"60. ..."`.
pub fn anchor_matches(heading_text: &str, anchor: &str) -> bool {
    match heading_text.strip_prefix(anchor) {
        None => false,
        Some(rest) => rest.chars().next().is_none_or(|c| !c.is_alphanumeric()),
    }
}

/// A scanned document (one that matched a genre in `docket.ncl`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Document {
    pub stem: DocStem,
    pub file: String,
    pub genre_path: String,
    pub headings: Vec<Heading>,
}

/// The full extraction result for a corpus: every claim and every scanned
/// document, prior to running any check.
#[derive(Debug, Clone, Default)]
pub struct Corpus {
    pub claims: Vec<Claim>,
    pub documents: Vec<Document>,
}

// --- §4.1 index output shape -------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct IndexClaim {
    pub file: String,
    pub line: usize,
    pub kind: String,
    pub evaluator: String,
    pub cites: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct IndexDocument {
    pub file: String,
    pub genre: String,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct Index {
    pub claims: BTreeMap<ClaimId, IndexClaim>,
    pub documents: BTreeMap<DocStem, IndexDocument>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cite_ref_parses_bare_claim_id() {
        assert_eq!(
            CiteRef::parse("lock-groundness"),
            CiteRef::Claim("lock-groundness".into())
        );
    }

    #[test]
    fn cite_ref_parses_document_anchor() {
        assert_eq!(
            CiteRef::parse("composition-model#6"),
            CiteRef::DocAnchor {
                stem: "composition-model".into(),
                anchor: "6".into()
            }
        );
    }

    #[test]
    fn cite_ref_display_round_trips() {
        assert_eq!(
            CiteRef::parse("lock-groundness").to_string(),
            "lock-groundness"
        );
        assert_eq!(
            CiteRef::parse("composition-model#6").to_string(),
            "composition-model#6"
        );
    }

    #[test]
    fn anchor_matches_prefix_with_non_alphanumeric_boundary() {
        assert!(anchor_matches("6. The fact-set", "6"));
        assert!(anchor_matches("2.4 Identity discipline", "2.4"));
        assert!(anchor_matches("6", "6")); // end-of-string boundary
    }

    #[test]
    fn anchor_matches_rejects_a_numeric_prefix_collision() {
        // The spec's own counter-example: "#6" must not match "## 60. ...".
        assert!(!anchor_matches("60. Something else", "6"));
    }

    #[test]
    fn anchor_matches_rejects_non_prefix() {
        assert!(!anchor_matches("The fact-set", "6"));
    }
}
