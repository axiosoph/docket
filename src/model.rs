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

/// Every Unicode combining mark (General_Category `Mn`, `Mc`, or `Me`,
/// Unicode 16.0.0) as sorted, half-open-inclusive codepoint ranges —
/// `github-slugger`'s own blacklist regex does not strip this class
/// (confirmed by parsing its actual published `regex.js`, not assumed:
/// none of `̀`–`ͯ` appears in any of its ~1000 strip-ranges),
/// so GitHub keeps a combining accent while `char::is_alphanumeric`
/// returns `false` for one — [`heading_slug`]'s own approximation
/// silently stripped every accent an NFD-composed heading carries
/// (`café` → `cafe`, the form many editors, filesystems, and
/// copy-pasted GitHub URLs actually produce), turning a real GitHub
/// anchor into a false `dangling-reference`.
///
/// Generated from Python 3.14's `unicodedata` (Unicode 16.0.0) by
/// listing every codepoint whose `unicodedata.category()` is `Mn`,
/// `Mc`, or `Me` and merging into contiguous ranges — the same
/// verification method as the `\0` NUL-escape fix this table's own
/// derivation needed (`github-slugger`'s regex source parses `\0` as
/// codepoint 0, not literal `'0'`; getting that wrong silently
/// corrupted every other range comparison downstream of it). Regenerate
/// by re-running that script against a newer Unicode Character Database
/// if `github-slugger` itself ever rebases onto one.
///
/// **Not a byte-perfect port, an approximation like `heading_slug`
/// itself already is — stated, not silent.** 38 of the 2501 codepoints
/// in this table (Unicode 16.0.0 Arabic Quranic annotation marks
/// U+0897–U+08D2, plus two Telugu/Kannada signs) are recent additions
/// `github-slugger`'s own blacklist has not yet caught up to and DOES
/// still strip — keeping them here means docket's slug diverges from
/// GitHub's on exactly those codepoints. Left as a residual rather than
/// carved out: unlike the class this table exists to fix (Latin,
/// Hebrew, Devanagari, Thai, and other in-use scripts' combining
/// marks — real, common heading content), a 38-codepoint sliver of
/// Quranic recitation notation is not a shape any of this tool's real
/// corpora carry, the same judgment MVP.md's residual note already
/// makes for the emoji gap below.
const COMBINING_MARK_RANGES: &[(u32, u32)] = &[
    (0x300, 0x36F),
    (0x483, 0x489),
    (0x591, 0x5BD),
    (0x5BF, 0x5BF),
    (0x5C1, 0x5C2),
    (0x5C4, 0x5C5),
    (0x5C7, 0x5C7),
    (0x610, 0x61A),
    (0x64B, 0x65F),
    (0x670, 0x670),
    (0x6D6, 0x6DC),
    (0x6DF, 0x6E4),
    (0x6E7, 0x6E8),
    (0x6EA, 0x6ED),
    (0x711, 0x711),
    (0x730, 0x74A),
    (0x7A6, 0x7B0),
    (0x7EB, 0x7F3),
    (0x7FD, 0x7FD),
    (0x816, 0x819),
    (0x81B, 0x823),
    (0x825, 0x827),
    (0x829, 0x82D),
    (0x859, 0x85B),
    (0x898, 0x89F),
    (0x8CA, 0x8E1),
    (0x8E3, 0x903),
    (0x93A, 0x93C),
    (0x93E, 0x94F),
    (0x951, 0x957),
    (0x962, 0x963),
    (0x981, 0x983),
    (0x9BC, 0x9BC),
    (0x9BE, 0x9C4),
    (0x9C7, 0x9C8),
    (0x9CB, 0x9CD),
    (0x9D7, 0x9D7),
    (0x9E2, 0x9E3),
    (0x9FE, 0x9FE),
    (0xA01, 0xA03),
    (0xA3C, 0xA3C),
    (0xA3E, 0xA42),
    (0xA47, 0xA48),
    (0xA4B, 0xA4D),
    (0xA51, 0xA51),
    (0xA70, 0xA71),
    (0xA75, 0xA75),
    (0xA81, 0xA83),
    (0xABC, 0xABC),
    (0xABE, 0xAC5),
    (0xAC7, 0xAC9),
    (0xACB, 0xACD),
    (0xAE2, 0xAE3),
    (0xAFA, 0xAFF),
    (0xB01, 0xB03),
    (0xB3C, 0xB3C),
    (0xB3E, 0xB44),
    (0xB47, 0xB48),
    (0xB4B, 0xB4D),
    (0xB55, 0xB57),
    (0xB62, 0xB63),
    (0xB82, 0xB82),
    (0xBBE, 0xBC2),
    (0xBC6, 0xBC8),
    (0xBCA, 0xBCD),
    (0xBD7, 0xBD7),
    (0xC00, 0xC04),
    (0xC3C, 0xC3C),
    (0xC3E, 0xC44),
    (0xC46, 0xC48),
    (0xC4A, 0xC4D),
    (0xC55, 0xC56),
    (0xC62, 0xC63),
    (0xC81, 0xC83),
    (0xCBC, 0xCBC),
    (0xCBE, 0xCC4),
    (0xCC6, 0xCC8),
    (0xCCA, 0xCCD),
    (0xCD5, 0xCD6),
    (0xCE2, 0xCE3),
    (0xCF3, 0xCF3),
    (0xD00, 0xD03),
    (0xD3B, 0xD3C),
    (0xD3E, 0xD44),
    (0xD46, 0xD48),
    (0xD4A, 0xD4D),
    (0xD57, 0xD57),
    (0xD62, 0xD63),
    (0xD81, 0xD83),
    (0xDCA, 0xDCA),
    (0xDCF, 0xDD4),
    (0xDD6, 0xDD6),
    (0xDD8, 0xDDF),
    (0xDF2, 0xDF3),
    (0xE31, 0xE31),
    (0xE34, 0xE3A),
    (0xE47, 0xE4E),
    (0xEB1, 0xEB1),
    (0xEB4, 0xEBC),
    (0xEC8, 0xECE),
    (0xF18, 0xF19),
    (0xF35, 0xF35),
    (0xF37, 0xF37),
    (0xF39, 0xF39),
    (0xF3E, 0xF3F),
    (0xF71, 0xF84),
    (0xF86, 0xF87),
    (0xF8D, 0xF97),
    (0xF99, 0xFBC),
    (0xFC6, 0xFC6),
    (0x102B, 0x103E),
    (0x1056, 0x1059),
    (0x105E, 0x1060),
    (0x1062, 0x1064),
    (0x1067, 0x106D),
    (0x1071, 0x1074),
    (0x1082, 0x108D),
    (0x108F, 0x108F),
    (0x109A, 0x109D),
    (0x135D, 0x135F),
    (0x1712, 0x1715),
    (0x1732, 0x1734),
    (0x1752, 0x1753),
    (0x1772, 0x1773),
    (0x17B4, 0x17D3),
    (0x17DD, 0x17DD),
    (0x180B, 0x180D),
    (0x180F, 0x180F),
    (0x1885, 0x1886),
    (0x18A9, 0x18A9),
    (0x1920, 0x193B),
    (0x1A17, 0x1A1B),
    (0x1A55, 0x1A5E),
    (0x1A60, 0x1A7C),
    (0x1A7F, 0x1A7F),
    (0x1AB0, 0x1ACE),
    (0x1B00, 0x1B04),
    (0x1B34, 0x1B44),
    (0x1B6B, 0x1B73),
    (0x1B80, 0x1B82),
    (0x1BA1, 0x1BAD),
    (0x1BE6, 0x1BF3),
    (0x1C24, 0x1C37),
    (0x1CD0, 0x1CD2),
    (0x1CD4, 0x1CE8),
    (0x1CED, 0x1CED),
    (0x1CF4, 0x1CF4),
    (0x1CF7, 0x1CF9),
    (0x1DC0, 0x1DFF),
    (0x20D0, 0x20F0),
    (0x2CEF, 0x2CF1),
    (0x2D7F, 0x2D7F),
    (0x2DE0, 0x2DFF),
    (0x302A, 0x302F),
    (0x3099, 0x309A),
    (0xA66F, 0xA672),
    (0xA674, 0xA67D),
    (0xA69E, 0xA69F),
    (0xA6F0, 0xA6F1),
    (0xA802, 0xA802),
    (0xA806, 0xA806),
    (0xA80B, 0xA80B),
    (0xA823, 0xA827),
    (0xA82C, 0xA82C),
    (0xA880, 0xA881),
    (0xA8B4, 0xA8C5),
    (0xA8E0, 0xA8F1),
    (0xA8FF, 0xA8FF),
    (0xA926, 0xA92D),
    (0xA947, 0xA953),
    (0xA980, 0xA983),
    (0xA9B3, 0xA9C0),
    (0xA9E5, 0xA9E5),
    (0xAA29, 0xAA36),
    (0xAA43, 0xAA43),
    (0xAA4C, 0xAA4D),
    (0xAA7B, 0xAA7D),
    (0xAAB0, 0xAAB0),
    (0xAAB2, 0xAAB4),
    (0xAAB7, 0xAAB8),
    (0xAABE, 0xAABF),
    (0xAAC1, 0xAAC1),
    (0xAAEB, 0xAAEF),
    (0xAAF5, 0xAAF6),
    (0xABE3, 0xABEA),
    (0xABED, 0xABED),
    (0xFB1E, 0xFB1E),
    (0xFE00, 0xFE0F),
    (0xFE20, 0xFE2F),
    (0x101FD, 0x101FD),
    (0x102E0, 0x102E0),
    (0x10376, 0x1037A),
    (0x10A01, 0x10A03),
    (0x10A05, 0x10A06),
    (0x10A0C, 0x10A0F),
    (0x10A38, 0x10A3A),
    (0x10A3F, 0x10A3F),
    (0x10AE5, 0x10AE6),
    (0x10D24, 0x10D27),
    (0x10D69, 0x10D6D),
    (0x10EAB, 0x10EAC),
    (0x10EFD, 0x10EFF),
    (0x10F46, 0x10F50),
    (0x10F82, 0x10F85),
    (0x11000, 0x11002),
    (0x11038, 0x11046),
    (0x11070, 0x11070),
    (0x11073, 0x11074),
    (0x1107F, 0x11082),
    (0x110B0, 0x110BA),
    (0x110C2, 0x110C2),
    (0x11100, 0x11102),
    (0x11127, 0x11134),
    (0x11145, 0x11146),
    (0x11173, 0x11173),
    (0x11180, 0x11182),
    (0x111B3, 0x111C0),
    (0x111C9, 0x111CC),
    (0x111CE, 0x111CF),
    (0x1122C, 0x11237),
    (0x1123E, 0x1123E),
    (0x11241, 0x11241),
    (0x112DF, 0x112EA),
    (0x11300, 0x11303),
    (0x1133B, 0x1133C),
    (0x1133E, 0x11344),
    (0x11347, 0x11348),
    (0x1134B, 0x1134D),
    (0x11357, 0x11357),
    (0x11362, 0x11363),
    (0x11366, 0x1136C),
    (0x11370, 0x11374),
    (0x113B8, 0x113C0),
    (0x113C2, 0x113C2),
    (0x113C5, 0x113C5),
    (0x113C7, 0x113CA),
    (0x113CC, 0x113D0),
    (0x113D2, 0x113D2),
    (0x113E1, 0x113E2),
    (0x11435, 0x11446),
    (0x1145E, 0x1145E),
    (0x114B0, 0x114C3),
    (0x115AF, 0x115B5),
    (0x115B8, 0x115C0),
    (0x115DC, 0x115DD),
    (0x11630, 0x11640),
    (0x116AB, 0x116B7),
    (0x1171D, 0x1172B),
    (0x1182C, 0x1183A),
    (0x11930, 0x11935),
    (0x11937, 0x11938),
    (0x1193B, 0x1193E),
    (0x11940, 0x11940),
    (0x11942, 0x11943),
    (0x119D1, 0x119D7),
    (0x119DA, 0x119E0),
    (0x119E4, 0x119E4),
    (0x11A01, 0x11A0A),
    (0x11A33, 0x11A39),
    (0x11A3B, 0x11A3E),
    (0x11A47, 0x11A47),
    (0x11A51, 0x11A5B),
    (0x11A8A, 0x11A99),
    (0x11C2F, 0x11C36),
    (0x11C38, 0x11C3F),
    (0x11C92, 0x11CA7),
    (0x11CA9, 0x11CB6),
    (0x11D31, 0x11D36),
    (0x11D3A, 0x11D3A),
    (0x11D3C, 0x11D3D),
    (0x11D3F, 0x11D45),
    (0x11D47, 0x11D47),
    (0x11D8A, 0x11D8E),
    (0x11D90, 0x11D91),
    (0x11D93, 0x11D97),
    (0x11EF3, 0x11EF6),
    (0x11F00, 0x11F01),
    (0x11F03, 0x11F03),
    (0x11F34, 0x11F42),
    (0x11F5A, 0x11F5A),
    (0x13440, 0x13440),
    (0x13447, 0x13455),
    (0x1611E, 0x1612F),
    (0x16AF0, 0x16AF4),
    (0x16B30, 0x16B36),
    (0x16F4F, 0x16F4F),
    (0x16F51, 0x16F87),
    (0x16F8F, 0x16F92),
    (0x16FE4, 0x16FE4),
    (0x16FF0, 0x16FF1),
    (0x1BC9D, 0x1BC9E),
    (0x1CF00, 0x1CF2D),
    (0x1CF30, 0x1CF46),
    (0x1D165, 0x1D169),
    (0x1D16D, 0x1D172),
    (0x1D17B, 0x1D182),
    (0x1D185, 0x1D18B),
    (0x1D1AA, 0x1D1AD),
    (0x1D242, 0x1D244),
    (0x1DA00, 0x1DA36),
    (0x1DA3B, 0x1DA6C),
    (0x1DA75, 0x1DA75),
    (0x1DA84, 0x1DA84),
    (0x1DA9B, 0x1DA9F),
    (0x1DAA1, 0x1DAAF),
    (0x1E000, 0x1E006),
    (0x1E008, 0x1E018),
    (0x1E01B, 0x1E021),
    (0x1E023, 0x1E024),
    (0x1E026, 0x1E02A),
    (0x1E08F, 0x1E08F),
    (0x1E130, 0x1E136),
    (0x1E2AE, 0x1E2AE),
    (0x1E2EC, 0x1E2EF),
    (0x1E4EC, 0x1E4EF),
    (0x1E5EE, 0x1E5EF),
    (0x1E8D0, 0x1E8D6),
    (0x1E944, 0x1E94A),
    (0xE0100, 0xE01EF),
];

/// Whether `c` falls in [`COMBINING_MARK_RANGES`] — binary search over a
/// sorted, non-overlapping table.
fn is_combining_mark(c: char) -> bool {
    let cp = c as u32;
    COMBINING_MARK_RANGES
        .binary_search_by(|&(lo, hi)| {
            if cp < lo {
                std::cmp::Ordering::Greater
            } else if cp > hi {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Equal
            }
        })
        .is_ok()
}

/// A real, GitHub-style heading anchor ("slug"): lowercase, drop every
/// character that is neither a Unicode letter/digit/combining-mark nor a
/// space, hyphen, or underscore (dropped outright, never replaced —
/// `"a/b"` collapses to `"ab"`, not `"a-b"`; `"1.5 Foo"` collapses to
/// `"15-foo"`, not `"1-5-foo"`), then turn each surviving space into a
/// hyphen (one hyphen per space — consecutive spaces are never
/// collapsed). This is what an ordinary relative markdown link's
/// `#fragment` names on GitHub, and verified directly against this
/// project's own real corpus: every existing
/// `composition-model.md#3-composition-merge-is-a-partial-commutative-monoid`-shaped
/// link a document already carries is reproduced by this function
/// character-for-character. See MVP.md's residual note for exactly how
/// this approximates upstream's real algorithm (`github-slugger`, a
/// several-thousand-codepoint Unicode blacklist) rather than porting it
/// verbatim, and where the two can diverge — including why combining
/// marks ([`is_combining_mark`], [`COMBINING_MARK_RANGES`]) are kept
/// alongside `char::is_alphanumeric` rather than folded into it: a
/// combining accent is not itself alphanumeric by Unicode's own
/// category, but `github-slugger`'s blacklist does not strip it either.
///
/// Does **not** deduplicate — see [`assign_heading_slugs`] for the
/// per-document dedup pass GitHub itself performs.
pub fn heading_slug(text: &str) -> String {
    let lower = text.to_lowercase();
    let kept: String = lower
        .chars()
        .filter(|&c| {
            c == ' ' || c == '-' || c == '_' || c.is_alphanumeric() || is_combining_mark(c)
        })
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
    fn heading_slug_keeps_an_nfd_composed_combining_accent() {
        // The real defect this table exists to fix: `é` written the way
        // many editors, filesystems, and copy-pasted GitHub URLs actually
        // produce it — NFD-composed, `e` (U+0065) followed by a bare
        // COMBINING ACUTE ACCENT (U+0301) — rather than the single
        // precomposed `é` (U+00E9). `char::is_alphanumeric` returns
        // `false` for U+0301 on its own, so before this table existed,
        // `heading_slug` silently dropped it — `github-slugger`'s own
        // blacklist does not (confirmed by parsing its published regex,
        // not assumed), so GitHub renders `café-notes`, not `cafe-notes`.
        let nfd_cafe = "Caf\u{65}\u{301} notes"; // "Café notes", NFD form
        assert_eq!(heading_slug(nfd_cafe), "caf\u{65}\u{301}-notes");
    }

    #[test]
    fn heading_slug_keeps_hebrew_cantillation_marks() {
        // Not only the Latin combining-diacritics block: any script's
        // combining marks are affected the same way, and the fix (a
        // Unicode General_Category Mn/Mc/Me table, not a Latin-only
        // range) closes the whole class in one pass. A real reproduction,
        // not merely a documented block boundary: U+0591 (HEBREW ACCENT
        // ETNAHTA) is `Mn` and outside github-slugger's blacklist, the
        // same shape as the NFD-`é` case above.
        let with_etnahta = "\u{5D0}\u{591}\u{5D1}"; // א ETNAHTA ב
        assert_eq!(heading_slug(with_etnahta), "\u{5D0}\u{591}\u{5D1}");
    }

    #[test]
    fn is_combining_mark_is_false_for_an_ordinary_letter() {
        // The false-positive floor: an ordinary base letter must never be
        // read as a combining mark, or every alphanumeric character in
        // `heading_slug` would be double-counted through both branches
        // harmlessly today, but the predicate itself would be wrong.
        assert!(!is_combining_mark('e'));
        assert!(!is_combining_mark('5'));
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
