//! Core data types shared by extraction, checks, the index, and blast radius.
//!
//! These mirror MVP.md §1.2 (claim fields), §1.3 (reference syntax), and
//! §4.1 (index shape) directly — the vocabulary here is the spec's
//! vocabulary, not an invented abstraction over it.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// A claim id: a bracketed kebab-case token, without the brackets.
pub type ClaimId = String;

/// A document identifier: the corpus-relative path with the `.md`
/// extension removed (MVP.md §1.3). Unique by construction — two files
/// can share a basename (`docs/models/lean/README.md` and
/// `docs/models/tla/README.md`) but never a path.
pub type DocPath = String;

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

/// MVP.md §2: Divio's four documentation quadrants — which question a
/// genre's documents answer. Orthogonal to `kind`: `kind` says what a
/// single claim asserts; `quadrant` says what job the genre as a whole
/// does, and is what makes the genre taxonomy comparable across corpora
/// (a project's own genre names, e.g. `docs/specs/**`, do not travel).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Quadrant {
    Tutorial,
    HowTo,
    Reference,
    Explanation,
}

impl Quadrant {
    pub fn as_str(&self) -> &'static str {
        match self {
            Quadrant::Tutorial => "tutorial",
            Quadrant::HowTo => "how-to",
            Quadrant::Reference => "reference",
            Quadrant::Explanation => "explanation",
        }
    }
}

/// MVP.md §1.2: the eight permitted evaluator names. Unused today — every
/// call site (extract.rs, checks.rs, run.rs) carries `evaluator` as the
/// raw `String` MVP.md's contract already validates, so this mirror has
/// no consumer; kept in step with the contract anyway rather than left to
/// drift further out of sync (it was already missing `Type` before this
/// change touched it).
///
/// `Review` is not a lower variant than `None`: see `claim.ncl`'s
/// `EvaluatorPred` doc comment for why review is a different evidence
/// species (a vouch) rather than a seventh rung on the mechanical scale.
/// `Absent` is likewise outside the strength order — it corroborates the
/// opposite predicate (a literal's non-occurrence, not a behavior's
/// occurrence) rather than ranking below `example` — see `claim.ncl` and
/// `run.rs`'s module docs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Evaluator {
    Proof,
    ModelCheck,
    Type,
    PropertyTest,
    Test,
    Example,
    Absent,
    Review,
    None,
}

/// MVP.md §1.3: a `depends`/`because` entry is either a claim id or a
/// document anchor.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum CiteRef {
    /// A bare kebab-case claim id, e.g. `lock-groundness`.
    Claim(ClaimId),
    /// `<doc-path>#<anchor>`, e.g. `docs/models/composition-model#6`.
    DocAnchor { path: DocPath, anchor: String },
}

impl CiteRef {
    /// Parse the `<doc-path>#<anchor>` / bare-id surface syntax shared by
    /// `depends`/`because` entries (MVP.md §1.3) and normalized prose link
    /// targets (§3, C5). This is a pure split, not a resolution — a raw
    /// entry is already in final path form by the time it reaches this
    /// crate (C1 enforces the shape), and a prose href is resolved to the
    /// same form beforehand (see `checks::normalize_prose_link`).
    pub fn parse(raw: &str) -> CiteRef {
        match raw.split_once('#') {
            Some((path, anchor)) => CiteRef::DocAnchor {
                path: path.to_string(),
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
            CiteRef::DocAnchor { path, anchor } => write!(f, "{path}#{anchor}"),
        }
    }
}

/// The three reference kinds (`.ledger/2026-07-30-reference-kinds-and-document-resolution.md`,
/// R1). Only `depends` and `because` are represented here — a **bare**
/// reference is, by design, not a value of this type at all: R3 defines it
/// as "a prose link that is not declared", so it has no field to populate
/// and no edge in the citation graph. Adding a third variant would let an
/// author *assert* bareness, which is meaningless — bareness is the
/// absence of a declaration, not a declaration of absence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RefKind {
    /// The claim's truth or meaning requires the target. Dangling ⇒ the
    /// claim is broken (C4).
    Depends,
    /// The claim's justification is the target. Dangling ⇒ the claim still
    /// stands, but its stated reason is orphaned (`orphaned-because`).
    Because,
}

impl RefKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            RefKind::Depends => "depends",
            RefKind::Because => "because",
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
    pub depends: Vec<String>,
    pub because: Vec<String>,
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
    /// `depends` entries parsed via [`CiteRef::parse`], best-effort. A
    /// dangling `depends` target means this claim is broken (C4).
    pub depends: Vec<CiteRef>,
    /// `because` entries parsed via [`CiteRef::parse`], best-effort. A
    /// dangling `because` target means this claim's stated reason is
    /// orphaned, not that the claim is false (`orphaned-because`).
    pub because: Vec<CiteRef>,
    /// Raw, un-normalized link targets found in the claim's prose body
    /// (§3, C5's `L`, before restriction to resolving targets). External
    /// URLs are dropped at extraction time since C5 ignores them
    /// unconditionally; everything else is a real markdown href (a
    /// relative path, possibly with a `#fragment`) that checks.rs
    /// resolves against the corpus's documents and claim ids.
    pub prose_links: Vec<String>,
    /// Inline code span (`` `…` ``) text found in the claim's prose body,
    /// same scope as `prose_links`. Exists for the `absent-marker-stale`
    /// check (`src/absence.rs`): an absence claim's marker names a
    /// literal explicitly (never a region of prose,
    /// `.ledger/2026-08-05-references-that-leave-the-register.md` O3), so
    /// confirming the marker still corresponds to something the prose
    /// actually discusses means confirming that literal is still one of
    /// the code spans the claim's own prose carries — the same adjacency
    /// check a human proofreader would make by eye.
    pub prose_code: Vec<String>,
}

impl Claim {
    /// Every reference this claim declares, kind attached — the union
    /// `depends ∪ because` that forms real edges in the citation graph
    /// (blast, C4/`orphaned-because`, C5's subset check). A **bare**
    /// reference is, by design, absent from this iterator entirely: R3
    /// (`.ledger/2026-07-30-reference-kinds-and-document-resolution.md`)
    /// defines it as an undeclared prose link, so it never became a
    /// `CiteRef` in the first place.
    pub fn refs(&self) -> impl Iterator<Item = (RefKind, &CiteRef)> {
        self.depends
            .iter()
            .map(|c| (RefKind::Depends, c))
            .chain(self.because.iter().map(|c| (RefKind::Because, c)))
    }
}

/// A heading found anywhere in a scanned document, kept for anchor
/// resolution (`<doc-path>#<anchor>`, §1.3). `text` is already stripped of
/// `#` markers and leading whitespace — that's how pulldown-cmark hands us
/// heading content — so it's ready for [`anchor_matches`] as-is. `slug` is
/// the real, GitHub-style anchor a renderer and an ordinary prose link
/// both use — see [`heading_slug`] — computed and deduplicated per
/// document at extraction time ([`crate::extract::extract_document`]),
/// since GitHub's own dedup counter resets per document too.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Heading {
    pub level: u8,
    pub text: String,
    pub line: Line,
    pub slug: String,
}

/// A real, GitHub-style heading anchor ("slug"): lowercase, drop every
/// character that is neither a Unicode letter/digit nor a space, hyphen,
/// or underscore (dropped outright, never replaced — `"a/b"` collapses to
/// `"ab"`, not `"a-b"`; `"1.5 Foo"` collapses to `"15-foo"`, not
/// `"1-5-foo"`), then turn each surviving space into a hyphen (one hyphen
/// per space — consecutive spaces are never collapsed). This is what an
/// ordinary relative markdown link's `#fragment` names on GitHub, and
/// verified directly against this project's own real corpus: every
/// existing `composition-model.md#3-composition-merge-is-a-partial-commutative-monoid`-shaped
/// link a document already carries is reproduced by this function
/// character-for-character. See MVP.md's residual note for exactly how
/// this approximates upstream's real algorithm (`github-slugger`, a
/// several-thousand-codepoint Unicode blacklist) rather than porting it
/// verbatim, and where the two can diverge.
///
/// Does **not** deduplicate — see [`assign_heading_slugs`] for the
/// per-document dedup pass GitHub itself performs.
pub fn heading_slug(text: &str) -> String {
    let lower = text.to_lowercase();
    let kept: String = lower
        .chars()
        .filter(|&c| c == ' ' || c == '-' || c == '_' || c.is_alphanumeric())
        .collect();
    kept.replace(' ', "-")
}

/// Assign a unique slug to each heading text, in document order —
/// `github-slugger`'s own dedup rule, ported faithfully (verified against
/// its published source rather than recalled): the first heading with a
/// given base slug keeps it bare; every later heading whose *already*
/// hyphen-suffixed candidate collides with a slug some earlier heading
/// was assigned gets the next `-N` for that same base, so three headings
/// all slugging to `"foo"` become `foo`, `foo-1`, `foo-2` — counted
/// per-base, not by a single corpus-wide counter, and checked against
/// every slug assigned so far (not only same-base ones), the same two
/// properties the upstream implementation has.
pub fn assign_heading_slugs<'a>(texts: impl Iterator<Item = &'a str>) -> Vec<String> {
    let mut used: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut counters: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
    texts
        .map(|text| {
            let base = heading_slug(text);
            let mut slug = base.clone();
            while used.contains(&slug) {
                let counter = counters.entry(base.clone()).or_insert(0);
                *counter += 1;
                slug = format!("{base}-{counter}");
            }
            used.insert(slug.clone());
            slug
        })
        .collect()
}

/// MVP.md §1.3, "Anchor derivation": an anchor `A` matches a heading iff
/// the heading's text — after stripping `#` markers and leading
/// whitespace — begins with `A` followed by either end-of-string or a
/// non-alphanumeric character. So `6` matches `"6. The fact-set: ..."`
/// but not `"60. ..."`. This is `depends`/`because`'s own resolution rule
/// (deliberately number-based, not slug-based — see MVP.md §1.3) and is
/// untouched by [`heading_slug`]: the two are separate addressing schemes
/// for the same headings, not one generalized into the other.
pub fn anchor_matches(heading_text: &str, anchor: &str) -> bool {
    match heading_text.strip_prefix(anchor) {
        None => false,
        Some(rest) => rest.chars().next().is_none_or(|c| !c.is_alphanumeric()),
    }
}

/// A scanned document (one that matched a genre in `docket.ncl`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Document {
    /// The identifier other claims cite by (MVP.md §1.3): `file` with its
    /// `.md` extension removed.
    pub doc_path: DocPath,
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
    pub depends: Vec<String>,
    pub because: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct IndexDocument {
    pub file: String,
    pub genre: String,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct Index {
    pub claims: BTreeMap<ClaimId, IndexClaim>,
    pub documents: BTreeMap<DocPath, IndexDocument>,
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
            CiteRef::parse("docs/models/composition-model#6"),
            CiteRef::DocAnchor {
                path: "docs/models/composition-model".into(),
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
            CiteRef::parse("docs/models/composition-model#6").to_string(),
            "docs/models/composition-model#6"
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

    // --- heading_slug: pinned against this project's own real corpus, not
    // recalled — most cases below are exact heading/link pairs that
    // already exist in `/var/home/nrd/git/github.com/axiosoph/axios`; the
    // two that aren't (non-ASCII punctuation, underscore preservation) are
    // general property checks against `github-slugger`'s published
    // source, since the corpus has no live link to a heading of that
    // exact shape to pin against instead. ---------------------------

    #[test]
    fn heading_slug_matches_a_real_numbered_heading() {
        // composition-model.md:211's `## 3. Composition merge is a partial
        // commutative monoid`, cited from execution-model.md exactly this
        // way.
        assert_eq!(
            heading_slug("3. Composition merge is a partial commutative monoid"),
            "3-composition-merge-is-a-partial-commutative-monoid"
        );
    }

    #[test]
    fn heading_slug_drops_a_period_without_a_separator() {
        // execution-model.md:204's `### 1.5 The two strata of intent` —
        // the internal `.` is dropped outright, not turned into a hyphen,
        // so `1.5` collapses to `15`.
        assert_eq!(
            heading_slug("1.5 The two strata of intent"),
            "15-the-two-strata-of-intent"
        );
    }

    #[test]
    fn heading_slug_drops_a_slash_without_a_separator() {
        // lock-file-schema.md:217's
        // `## `[sets]` under the spine/cloud split (ADR-0009)` — the `/`
        // is dropped, not replaced, so "spine" and "cloud" concatenate
        // into "spinecloud" rather than "spine-cloud".
        assert_eq!(
            heading_slug("[sets] under the spine/cloud split (ADR-0009)"),
            "sets-under-the-spinecloud-split-adr-0009"
        );
    }

    #[test]
    fn heading_slug_keeps_an_existing_hyphen_and_drops_a_colon() {
        // composition-model.md:557's
        // `### The cloud: a snapshot name for the fact-set`.
        assert_eq!(
            heading_slug("The cloud: a snapshot name for the fact-set"),
            "the-cloud-a-snapshot-name-for-the-fact-set"
        );
    }

    #[test]
    fn heading_slug_folds_inline_code_and_brackets() {
        // adr/0009-atom-composition-plane.md:217's
        // `### 6. The store is a flat, content-addressed keyspace, with
        // every index derived [acp-store]` — pulldown-cmark hands the
        // trailing bracket id to `Heading.text` like any other word (no
        // backticks survive an inline code span either), and the comma is
        // dropped, not replaced.
        assert_eq!(
            heading_slug(
                "6. The store is a flat, content-addressed keyspace, with every index derived [acp-store]"
            ),
            "6-the-store-is-a-flat-content-addressed-keyspace-with-every-index-derived-acp-store"
        );
    }

    #[test]
    fn heading_slug_strips_em_dashes_and_section_signs_without_a_separator() {
        // Real corpus headings carry these (e.g. `## §1 — Layer
        // Architecture`, `ion-eos-contract.md`): non-ASCII punctuation the
        // upstream algorithm's blacklist also strips, confirmed against
        // its published regex rather than assumed. Two adjacent spaces
        // (one on each side of the deleted dash) become two literal
        // hyphens — consecutive hyphens are never collapsed.
        assert_eq!(
            heading_slug("§1 — Layer Architecture"),
            "1--layer-architecture"
        );
    }

    #[test]
    fn heading_slug_lowercases_and_keeps_underscore() {
        assert_eq!(
            heading_slug("`!Send` Threading_Model"),
            "send-threading_model"
        );
    }

    #[test]
    fn assign_heading_slugs_dedupes_in_document_order() {
        // github-slugger's own dedup: first occurrence bare, then -1, -2,
        // … — verified against its published `BananaSlug.slug()` source.
        let texts = ["Foo", "Foo", "Foo", "Bar"];
        assert_eq!(
            assign_heading_slugs(texts.iter().copied()),
            vec!["foo", "foo-1", "foo-2", "bar"]
        );
    }

    #[test]
    fn assign_heading_slugs_does_not_let_a_real_collision_race_a_generated_one() {
        // "Foo" (dup 1 of "foo") then a heading whose OWN literal text
        // slugs to "foo-1" must not collide silently with the generated
        // suffix — the generated one is checked against every slug
        // assigned so far, so it skips past the literal one.
        let texts = ["Foo", "Foo 1", "Foo"];
        let slugs = assign_heading_slugs(texts.iter().copied());
        assert_eq!(slugs[0], "foo");
        assert_eq!(slugs[1], "foo-1");
        assert_ne!(slugs[2], slugs[1]);
        assert_eq!(slugs[2], "foo-2");
    }
}
