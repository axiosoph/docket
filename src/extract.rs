//! The extractor: MVP.md §1.1 (claim block syntax + id assignment), the
//! C5 prose-link scope (§3), and the `normative-prose` own-voice scan
//! (§2/§3: a genre declaring `kinds = []` permits no claim blocks, which
//! already means no RFC-2119 keyword may appear in that genre's own
//! voice either — see checks.rs, which is where genres come into view).
//!
//! Parses markdown to pulldown-cmark's event tree and walks it — no
//! regular expressions over prose (MVP.md §7). The hand-rolled tokenizers
//! here ([`bracket_kebab_id`], [`ascii_words`]) operate on an
//! already-isolated string a tree walk has produced, not on document
//! structure, which is the distinction MVP.md §7 draws.

use crate::model::{CiteRef, Claim, ClaimId, Heading, Line, RawClaimBlock, assign_heading_slugs};
use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};

/// A `claim` fence with no preceding bracket-kebab heading in the same
/// file (MVP.md §1.1: "A claim block with no such preceding heading in
/// the same file is an error (`orphan-claim`).").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrphanClaim {
    pub file: String,
    pub line: Line,
}

/// An RFC-2119 keyword found in a document's own voice: outside a block
/// quote at any nesting depth, an inline code span, or a fenced (or
/// indented) code block. Extraction is genre-agnostic — every occurrence
/// in every scanned document is collected here; whether it is a
/// violation depends on the document's genre, which only checks.rs's
/// `normative-prose` check has in view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormativeOccurrence {
    pub file: String,
    pub line: Line,
    /// The canonical keyword text, e.g. `"MUST"` or `"MUST NOT"`.
    pub keyword: &'static str,
}

/// A recognized id definition (heading-form or bold-form — see
/// [`bracket_kebab_id`] / [`scan_interstitial_chunk`]) with no
/// `claim` block that claimed it (§1.1's ownership rule, generalized to
/// both forms). This is the coverage-count deliverable: a real-corpus run
/// measured 418 recognized definitions, only 18 with a registered block —
/// this is the other 400, discovered rather than counted by hand.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnregisteredDefinition {
    pub file: String,
    pub line: Line,
    pub id: ClaimId,
}

/// A bracketed token in a recognized definition position (heading- or
/// bold-form) whose inner content fails the id grammar — see
/// [`malformed_bracket_id`] for the exact predicate and why it exists.
/// Distinct from [`UnregisteredDefinition`]: the remedy here is renaming
/// the id, not writing a claim block, so it gets its own diagnostic
/// (`.ledger/2026-08-04-malformed-ids-are-silently-invisible.md`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MalformedId {
    pub file: String,
    pub line: Line,
    /// The bracket's raw inner text — not a [`ClaimId`], since by
    /// definition it failed the grammar that type implies.
    pub id: String,
}

/// A markdown link found anywhere in a scanned document — the whole
/// document's link surface, independent of any claim's prose scope.
/// Deliberately a second, document-level collection alongside
/// `Claim::prose_links`, not a repurposing of it:
/// `.ledger/2026-08-05-links-are-document-facts-not-claim-attributes.md`
/// names moving `prose_links` itself to document scope as a larger,
/// separate change (C5's claim-scoping stays load-bearing for C5); this
/// struct exists so `unreachable-reference` (a check with no claim in
/// view at all — a document with zero claims still has links) can see
/// every link without touching that.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentLink {
    pub file: String,
    pub line: Line,
    /// The raw href exactly as written. External URLs are already
    /// excluded ([`is_external`]), the same filter `prose_links` applies —
    /// a scheme-qualified target is never corpus-relative, so it is never
    /// path-shaped for any consumer of this list.
    pub dest: String,
}

#[derive(Debug, Clone, Default)]
pub struct ExtractResult {
    pub headings: Vec<Heading>,
    pub claims: Vec<Claim>,
    pub orphan_claims: Vec<OrphanClaim>,
    pub normative_occurrences: Vec<NormativeOccurrence>,
    pub unregistered_definitions: Vec<UnregisteredDefinition>,
    pub malformed_ids: Vec<MalformedId>,
    pub links: Vec<DocumentLink>,
    /// Every inline code span in the document whose content might name a
    /// path — the widened `unreachable-reference` surface
    /// (`gitignore::find_unreachable_references`): a bare mention like
    /// `` `.ledger/2026-08-05-foo.md` `` is a pointer a reader cannot
    /// follow just as much as a markdown link is, even though it carries
    /// none of a link's syntax. Never consulted by `dangling-reference`
    /// or C5 — see this field's construction site for why.
    pub code_references: Vec<DocumentLink>,
}

/// Byte-offset -> 1-indexed line number, built once per document.
struct LineIndex(Vec<usize>);

impl LineIndex {
    fn new(src: &str) -> Self {
        let mut starts = vec![0usize];
        starts.extend(
            src.bytes()
                .enumerate()
                .filter(|&(_, b)| b == b'\n')
                .map(|(i, _)| i + 1),
        );
        LineIndex(starts)
    }

    fn line_of(&self, offset: usize) -> Line {
        // Number of line-starts at or before `offset` is exactly the
        // 1-indexed line number, since starts[0] == 0.
        Line(self.0.partition_point(|&s| s <= offset))
    }
}

/// Whether `text` is exactly a bracketed token, e.g. `[my-id]` — the
/// structural half of a definition (brackets, non-empty), independent of
/// whether the inner content is valid kebab-case. Returns the inner text,
/// without brackets. Shared by [`bracket_kebab_id`] (a real definition) and
/// [`malformed_bracket_id`] (one that fails the id grammar) so the two
/// stay structurally identical apart from the grammar check itself.
fn bracket_token(text: &str) -> Option<&str> {
    let inner = text.strip_prefix('[')?.strip_suffix(']')?;
    (!inner.is_empty()).then_some(inner)
}

/// MVP.md §1.1's id grammar — delegates to [`crate::model::is_valid_claim_id`],
/// the single source of truth `rename.rs`'s new-id validation shares.
fn is_kebab_case(inner: &str) -> bool {
    crate::model::is_valid_claim_id(inner)
}

/// Whether `text` is exactly a bracketed kebab-case token, e.g. `[my-id]`.
/// Returns the inner id, without brackets.
fn bracket_kebab_id(text: &str) -> Option<String> {
    let inner = bracket_token(text)?;
    is_kebab_case(inner).then(|| inner.to_string())
}

/// A heading's id: a bracketed kebab-case token at the very START of the
/// heading text, tolerating whatever follows the closing bracket.
///
/// Deliberately looser than [`bracket_kebab_id`] (which the bold- and
/// html-form definitions still use, and requires the *whole* text to be
/// the bracket): a heading is not the id, it CARRIES an id, and MVP.md
/// §1.1 never reserved the rest of the heading line for nothing else. A
/// real-corpus retirement convention writes `#### [my-id] — RETIRED
/// 2026-08-23`, and requiring an exact match made that suffix invisible
/// to id resolution — the heading fell out of `id_anchors` entirely, so
/// its OWN claim block was misattributed to the nearest PRECEDING
/// anchor, silently clobbering that claim in the final index (a
/// `BTreeMap` keyed by id — last write wins). Anchoring on the *leading*
/// bracket instead of the whole string fixes that without changing what
/// a claim id may contain: `[my-id]` still must satisfy
/// [`is_kebab_case`], and a heading not starting with `[` is still not a
/// definition at all.
fn heading_bracket_kebab_id(text: &str) -> Option<String> {
    let rest = text.strip_prefix('[')?;
    let inner = &rest[..rest.find(']')?];
    is_kebab_case(inner).then(|| inner.to_string())
}

/// Whether `text` is a bracketed token in definition position that FAILS
/// the id grammar — the `malformed-id` diagnostic's predicate
/// (`.ledger/2026-08-04-malformed-ids-are-silently-invisible.md`).
///
/// Structurally identical to [`bracket_kebab_id`] (bracket-wrapped,
/// non-empty) but for the grammar check itself, plus one further filter:
/// the inner text must carry **no whitespace**. That filter is what keeps
/// this from crying wolf on ordinary bracketed prose — `**[Note to
/// reader]**: ...` reads as a sentence, not an attempted id, and the
/// real-corpus defect this exists to catch (`[boundary-L1-concerns]`,
/// `[daemon-discovery-vN]`) never has a space in it: an id-shaped token
/// with a stray uppercase letter, underscore, or dot still reads as one
/// *word*. A multi-word bracket is prose; a one-word bracket that isn't
/// lowercase-kebab is far more likely a typo'd id than a coincidence
/// (MVP.md §1.1's own reasoning for the diagnostic: "prose rarely opens a
/// line with a bolded bracketed kebab-ish token followed by a colon").
/// Returns the offending inner text — not a [`ClaimId`], since by
/// definition it isn't one.
fn malformed_bracket_id(text: &str) -> Option<&str> {
    let inner = bracket_token(text)?;
    if is_kebab_case(inner) || inner.contains(char::is_whitespace) {
        None
    } else {
        Some(inner)
    }
}

/// The tag name of an HTML open tag's raw text (`` `<a id="x">` `` ->
/// `"a"`), or `None` if `tag` isn't shaped like an open tag at all.
/// Case-sensitive: every real corpus this project targets writes lowercase
/// tag names, and HTML tag names are case-insensitive by spec but this
/// scan is lexical, not a real HTML parser — see [`html_attr_value`]'s own
/// doc comment for the same boundary stated the same way.
fn html_open_tag_name(tag: &str) -> Option<&str> {
    let body = tag.strip_prefix('<')?;
    if body.starts_with('/') {
        return None;
    }
    let end = body.find(|c: char| c.is_whitespace() || c == '>' || c == '/')?;
    (!body[..end].is_empty()).then_some(&body[..end])
}

/// Whether `tag` is exactly a closing tag for `name` (`` `</a>` `` closes
/// `"a"`).
fn is_html_close_tag(tag: &str, name: &str) -> bool {
    tag.strip_prefix("</")
        .and_then(|rest| rest.strip_suffix('>'))
        .is_some_and(|inner| inner.trim().eq_ignore_ascii_case(name))
}

/// The raw value of attribute `name` in HTML open-tag text `tag` — the
/// `` `<a id="x">` ``-shaped string an `Event::InlineHtml`/`Event::Html`
/// hands over verbatim. `None` if `name` never appears as its own
/// whitespace-delimited attribute token (so `data-id="x"` never matches a
/// search for `id`, unlike a naive substring search) or appears without a
/// quoted value (a bare `id` or an unquoted `id=x`).
///
/// **A lexical scan, not an HTML parser — two residuals stated rather
/// than hidden**, the same class of limitation `gitignore.rs`'s backtick
/// scan and `absence.rs`'s literal search already carry: an attribute
/// value containing whitespace (`id="my id"`) is split at the space and
/// missed, and an unquoted or valueless `id` attribute is not recognized
/// at all. Every real anchor this project's own corpus and its dispatch
/// examples use is double-quoted kebab-case with no internal whitespace,
/// so neither residual is expected to matter in practice; both are named
/// in MVP.md.
fn html_attr_value<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let body = tag
        .strip_prefix('<')?
        .trim_end_matches('>')
        .trim_end_matches('/');
    // Skip the tag name itself (the first token) — only real attributes
    // after it are candidates, so a tag named e.g. `id` (not real HTML,
    // but not this scan's problem to rule out) can't self-match.
    let mut tokens = body.split_whitespace();
    tokens.next()?;
    for tok in tokens {
        let Some(rest) = tok.strip_prefix(name).and_then(|r| r.strip_prefix('=')) else {
            continue;
        };
        let quote = rest.chars().next()?;
        if quote != '"' && quote != '\'' {
            continue;
        }
        let inner = &rest[1..];
        if let Some(end) = inner.find(quote) {
            return Some(&inner[..end]);
        }
    }
    None
}

/// Bound, in source bytes, on the interstitial between a bold-form
/// definition's closing `**` and its terminating colon (the fourth of the
/// four properties that together identify a definition — line start, the
/// `**…**` wrapper, a bracketed kebab id inside it, and this; the other
/// three are checked by the caller). What sits in the interstitial is
/// deliberately unconstrained in *kind* — a parenthetical formula label
/// (`**[id]** (P8):`), an italicized revision note
/// (`**[id]** _(amended 2026-07-14)_:`), whatever a corpus grows into
/// next — but bounded in *length*, so the scan below never degrades into
/// an open-ended prose search. A real-corpus measurement of the italic
/// form found interstitials up to 88 bytes; this bound gives headroom
/// above that rather than sitting tight against it, because the two kinds
/// of error here are not symmetric: claim ids are unique corpus-wide
/// (enforced by `contracts/claim_apply.ncl`), so an over-matched
/// interstitial produces a loud duplicate-id finding naming both sites,
/// never a silently invented claim — while under-matching is exactly the
/// silently invisible definition this generalization exists to fix.
const MAX_INTERSTITIAL_LEN: usize = 128;

/// Outcome of scanning one interstitial chunk — an `Event::Text` chunk
/// immediately adjacent to where the interstitial left off — for the
/// colon that closes a bold-form definition.
enum InterstitialStep {
    /// The colon sits at this byte offset within the chunk.
    ResolvedAt(usize),
    /// No colon in this chunk, but nothing disqualifying either (no line
    /// break, budget not yet exhausted) — the interstitial may still
    /// resolve in a later, adjacent chunk.
    Continue,
    /// The chunk crossed a line break before any colon, or exhausted the
    /// remaining budget — the interstitial cannot close from here.
    Failed,
}

/// Scan `chunk` — up to `budget` bytes of it — for the interstitial's
/// terminating colon. A line break inside the (budget-limited) window
/// fails the chunk outright: "immediately followed by definitional
/// punctuation" (MVP.md §1.1) does not survive crossing a line. Operates
/// on an already-isolated string a tree walk produced, the same class of
/// hand-rolled tokenizing [`bracket_kebab_id`] and [`ascii_words`] do —
/// not a regex over document structure.
fn scan_interstitial_chunk(chunk: &str, budget: usize) -> InterstitialStep {
    let window_len = chunk.len().min(budget);
    let window = &chunk[..window_len];
    let scan = window.find('\n').map_or(window, |nl| &window[..nl]);
    if let Some(rel) = scan.find(':') {
        return InterstitialStep::ResolvedAt(rel);
    }
    if window.contains('\n') || window_len < chunk.len() {
        InterstitialStep::Failed
    } else {
        InterstitialStep::Continue
    }
}

/// RFC 2119's ten keywords, whole-word and all-caps only — capitalisation
/// is the only signal that distinguishes a normative keyword from the
/// ordinary English word. `MUST`/`SHOULD`/`SHALL` additionally combine
/// with an immediately-following `NOT` into the negated compound form
/// (`MUST NOT` is a prohibition, a different assertion than `MUST`); the
/// other four keywords have no such compound in the set. Returns the
/// canonical keyword text and whether `next` was consumed as part of it.
fn classify_keyword(word: &str, next: Option<&str>) -> Option<(&'static str, bool)> {
    match word {
        "MUST" if next == Some("NOT") => Some(("MUST NOT", true)),
        "MUST" => Some(("MUST", false)),
        "SHOULD" if next == Some("NOT") => Some(("SHOULD NOT", true)),
        "SHOULD" => Some(("SHOULD", false)),
        "SHALL" if next == Some("NOT") => Some(("SHALL NOT", true)),
        "SHALL" => Some(("SHALL", false)),
        "REQUIRED" => Some(("REQUIRED", false)),
        "RECOMMENDED" => Some(("RECOMMENDED", false)),
        "MAY" => Some(("MAY", false)),
        "OPTIONAL" => Some(("OPTIONAL", false)),
        _ => None,
    }
}

/// Maximal runs of ASCII alphabetic bytes in `text`, each paired with its
/// byte offset within `text`. Any other byte (digit, punctuation,
/// whitespace) is a word boundary, matching the boundary rule
/// [`crate::model::anchor_matches`] uses for heading prefixes — applied
/// here to tokenize a whole string rather than test one prefix.
fn ascii_words(text: &str) -> Vec<(&str, usize)> {
    let bytes = text.as_bytes();
    let mut words = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_alphabetic() {
            let start = i;
            while i < bytes.len() && bytes[i].is_ascii_alphabetic() {
                i += 1;
            }
            words.push((&text[start..i], start));
        } else {
            i += 1;
        }
    }
    words
}

/// Scan one `Event::Text` chunk for RFC-2119 keywords. `chunk_start` is
/// the chunk's byte offset in the source (from pulldown-cmark's
/// offset-iterator range), needed to map a match back to a line.
///
/// Negation (`MUST NOT`, `SHOULD NOT`, `SHALL NOT`) is only recognised
/// within a single chunk: `NOT` immediately following the modal verb in
/// the same run of plain text. A phrase split across chunks by inline
/// markup (e.g. `MUST **NOT**`, where `**` starts a new Text event)
/// reports the modal verb alone rather than the negated compound — the
/// occurrence is still caught, only the compound label is missed, for a
/// rare authoring pattern.
fn scan_normative_keywords(
    file: &str,
    text: &str,
    chunk_start: usize,
    line_index: &LineIndex,
) -> Vec<NormativeOccurrence> {
    let words = ascii_words(text);
    let mut out = Vec::new();
    let mut i = 0;
    while i < words.len() {
        let (word, offset) = words[i];
        let next = words.get(i + 1).map(|(w, _)| *w);
        if let Some((keyword, consumed_next)) = classify_keyword(word, next) {
            out.push(NormativeOccurrence {
                file: file.to_string(),
                line: line_index.line_of(chunk_start + offset),
                keyword,
            });
            i += if consumed_next { 2 } else { 1 };
        } else {
            i += 1;
        }
    }
    out
}

fn heading_level_u8(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

/// A markdown link target is "external" (ignored unconditionally by C5,
/// MVP.md §3) if it names a URL scheme rather than a corpus-relative path
/// or same-document fragment.
fn is_external(dest: &str) -> bool {
    dest.contains("://") || dest.starts_with("mailto:") || dest.starts_with("tel:")
}

fn string_array_field(mapping: Option<&serde_norway::Mapping>, field: &str) -> Vec<String> {
    mapping
        .and_then(|m| m.get(field))
        .and_then(|v| v.as_sequence())
        .map(|seq| {
            seq.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

fn parse_raw_claim_block(yaml: &str) -> RawClaimBlock {
    let value: Option<serde_norway::Value> = serde_norway::from_str(yaml).ok();
    let mapping = value.as_ref().and_then(|v| v.as_mapping());
    let kind = mapping
        .and_then(|m| m.get("kind"))
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let evaluator = mapping
        .and_then(|m| m.get("evaluator"))
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let depends = string_array_field(mapping, "depends");
    let because = string_array_field(mapping, "because");
    RawClaimBlock {
        yaml: yaml.to_string(),
        kind,
        evaluator,
        depends,
        because,
    }
}

struct RawHeading {
    level: u8,
    start: usize,
    end: usize,
    text: String,
}

struct RawLink {
    start: usize,
    dest: String,
}

/// An inline code span (`` `…` ``), captured the same way [`RawLink`]
/// captures a link — position plus content, scoped to a claim's prose
/// window by the same filter `prose_links` already applies. Feeds
/// `Claim::prose_code` (the `absent-marker-stale` check's adjacency
/// signal), nothing else; a heading's own code spans are unrelated and
/// already folded into `Event::Code`'s existing heading-text handling
/// below.
struct RawCode {
    start: usize,
    text: String,
}

struct RawBlock {
    start: usize,
    end: usize,
    is_claim: bool,
    yaml: String,
}

/// A recognized bold-form definition: `start` is the byte offset of the
/// opening `**` (used for the line-start check and as this definition's
/// position in the nearest-preceding-anchor search, §1.1's ownership
/// rule); `end` is the offset immediately after the matched definitional
/// punctuation, i.e. where the definition's own prose begins (the
/// bold-form analogue of a heading's `end`, used as C5's prose-scope
/// start).
struct RawBoldDef {
    start: usize,
    end: usize,
    id: ClaimId,
}

/// A line-start `**[...]**` bold span whose inner bracket text failed the
/// id grammar, once its definitional punctuation has confirmed it sits in
/// definition position — the bold-form analogue of [`RawBoldDef`], for
/// the `malformed-id` diagnostic rather than a real definition. `start` is
/// the opening `**`'s byte offset, used only to derive the line for
/// [`MalformedId`].
struct RawMalformedBold {
    start: usize,
    id: String,
}

/// What a closed line-start `**[...]**` bold span resolves to once its
/// definitional punctuation confirms it: a real id ([`RawBoldDef`]) or one
/// that fails the grammar ([`RawMalformedBold`], the `malformed-id`
/// diagnostic). Both share the same punctuation-hunting state machine
/// ([`PendingBold`]) — the id grammar is the only thing that decides which
/// outcome a given candidate resolves to.
enum BoldCandidate {
    Definition(ClaimId),
    Malformed(String),
}

/// A closed line-start `**[id]**` still hunting for its definitional
/// colon, carried across however many adjacent inline events the
/// interstitial spans. A colon-bearing `Event::Text` immediately after
/// the bold span resolves it in one step (the direct-colon and plain
/// parenthetical forms); a markup-wrapped aside (`_(amended …)_`)
/// instead arrives from pulldown-cmark as a `Start`/`Text`/`End` triple,
/// so this state persists across that whole span rather than only
/// checking the single next event.
struct PendingBold {
    /// Byte offset of the definition's opening `**`.
    start: usize,
    /// Byte offset where the interstitial began (`strong_end`) — budgets
    /// against [`MAX_INTERSTITIAL_LEN`] are measured from here.
    interstitial_start: usize,
    /// Byte offset the next adjacent event must start at. A gap (a
    /// non-adjacent event, or an event kind that can't continue the
    /// interstitial) abandons this candidate.
    cursor: usize,
    candidate: BoldCandidate,
    /// `>0` while inside a balanced inline wrapper whose whole span was
    /// already pre-consumed by jumping `cursor` to its `Start` event's
    /// `range.end` — pulldown-cmark's offset iterator gives a container's
    /// `Start` and `End` the same full-span range (the same fact
    /// `extract_document`'s heading/code-block capture already relies
    /// on), so one hop skips the wrapper entirely. Events inside are
    /// ignored outright, never scanned for a colon: the wrapper's content
    /// is unconstrained, the same rule the parenthetical form already
    /// applies to its own `(...)`, generalized from one wrapper syntax to
    /// any of them (emphasis today; whatever a corpus grows into next)
    /// rather than special-cased per syntax.
    skip_depth: u32,
}

/// A recognized `<a id="claim-id"></a>` definition — the third anchor
/// form, invisible in rendered output by design (the head's ruling: claim
/// ids must not pollute user-facing documentation, and `<a
/// id="…"></a>` is the only markdown-legal construct that is both
/// invisible to a reader and a genuine link target, unlike `<!--
/// comment -->` which renders nothing and therefore anchors nothing
/// either). `start` is the opening tag's byte offset (used for the
/// nearest-preceding-anchor search, same as the other two forms); `end`
/// is immediately after the closing `</a>`, the html-form analogue of a
/// bold definition's own `end` — where the definition's own prose scope
/// begins.
struct RawHtmlDef {
    start: usize,
    end: usize,
    id: ClaimId,
}

/// A closed, immediately-paired `<a id="…"></a>` whose id failed the
/// grammar — including empty (`` `id=""` ``), which routes here rather
/// than being silently ignored the way an empty bracket is: an author who
/// wrote `id=""` was attempting a definition, so silence would hide
/// exactly the defect `malformed-id` exists to surface. The html-form
/// analogue of [`RawMalformedBold`].
struct RawMalformedHtml {
    start: usize,
    id: String,
}

/// What a closed `<a id="…">` open tag resolves to once its id is read —
/// mirrors [`BoldCandidate`], one grammar check deciding
/// [`RawHtmlDef`]/[`RawMalformedHtml`]. Resolution itself still needs the
/// immediately-adjacent `</a>` confirming the pair is closed and empty
/// ([`PendingHtml`]) — an unclosed `<a id="…">`, or one with real content
/// before its `</a>`, is never a candidate at all: it is not the
/// invisible-marker shape this form exists for, the same way an
/// unpunctuated bold span is never a bold-form candidate.
enum HtmlCandidate {
    Definition(ClaimId),
    Malformed(String),
}

/// An `<a id="…">` open tag awaiting its immediately-adjacent `</a>` —
/// the html-form analogue of [`PendingBold`], simpler because there is no
/// interstitial to hunt through: the NEXT event must be the closing tag,
/// starting exactly where the open tag ended, or this candidate is
/// abandoned outright (never reported as malformed — an author who left
/// a real anchor open, or wrote actual content inside it, was not
/// necessarily attempting a docket definition at all).
struct PendingHtml {
    start: usize,
    end_of_open: usize,
    candidate: HtmlCandidate,
}

/// A recognized id definition, any of the three forms, merged into one
/// position-ordered stream for §1.1's "nearest preceding [id]" ownership
/// search — generalized from heading-only to whichever form is nearer,
/// exactly the way a deeper heading already wins over a shallower one.
/// `idx` indexes back into `raw_headings` / `raw_bold_defs` /
/// `raw_html_defs` so the claim-building loop can recover the
/// form-specific fields (heading level for its same-or-higher-level C5
/// scope close; the bold/html definition's own `end` for its scope
/// start).
enum AnchorKind {
    Heading(usize),
    Bold(usize),
    Html(usize),
}

struct IdAnchor {
    start: usize,
    id: ClaimId,
    kind: AnchorKind,
}

/// Whether html anchor `[def_start, def_end)` sits immediately beside a
/// heading — nothing but whitespace between them, in EITHER order (the
/// anchor may precede or follow its heading) — and if so, the NEAREST such
/// heading's index into `raw_headings`.
///
/// **Why this reclassification exists, not merely an optimization:** an
/// html anchor is positionally polymorphic in a way bold-form never is.
/// A bold span (`**[id]**: text`) is inherently inline — there is never
/// an adjacent heading it could name, so treating it as a section marker
/// would be meaningless. An html anchor beside a heading, by contrast,
/// genuinely names that SECTION, and discarding the adjacency
/// information (treating it as merely inline) produces a real defect:
/// `inline_scope_end` closes at the very next heading, which — when the
/// anchor sits immediately before its own heading — is that heading
/// itself, collapsing `[scope_start, scope_end)` to nothing and silently
/// dropping every link and code span in the section the author plainly
/// meant to claim. A free-standing anchor with no adjacent heading has
/// no such information to discard, and stays genuinely inline
/// (`AnchorKind::Html`, `inline_scope_end`) — this function is what
/// decides which case a given anchor is in, checked once at `id_anchors`
/// construction rather than folded into the scope-computation match arm.
///
/// **Nearest, not first.** A whitespace-only, no-body-prose heading
/// directly beside the anchor (an empty sibling immediately before it, or
/// a parent immediately before a subheading right after it) can satisfy
/// BOTH sides of the predicate for two different headings at once — the
/// earlier of the two is not necessarily the one the anchor names. Picking
/// the first match in document order silently mis-scoped it two distinct
/// ways: binding to an empty preceding sibling with real content on the
/// other side collapses the scope to nothing (that heading's own scope
/// closes at the very next heading — the one the anchor actually meant);
/// binding to an empty parent instead of its immediately-following child
/// over-widens the scope through every sibling subsection the child's own
/// narrower scope would have excluded. Comparing the whitespace-gap length
/// on each side and taking the smaller fixes both.
///
/// **The two sides are not measured the same way, so the raw gap needs a
/// +1 correction on one of them before comparing.** Confirmed directly
/// against `pulldown_cmark`'s own offset iterator, not assumed: a
/// heading's `Range` always absorbs its own line-terminating `\n` (`"##
/// A\n"`, not `"## A"`), but a heading's `Range` never absorbs anything
/// BEFORE it. So for the identical author-visible gap (say, one blank
/// line) on each side, "heading precedes the anchor" measures one byte
/// SHORTER than "anchor precedes the heading" purely as an artifact of
/// which side's newline got absorbed into whose range — not because the
/// author placed the anchor any closer to one heading than the other.
/// Left uncorrected, that phantom byte silently and systematically
/// prefers the PRECEDING heading on every real tie, which is exactly the
/// wrong direction for both defects above (both need the FOLLOWING
/// heading to win the tie). Adding 1 to the "heading precedes anchor"
/// side's raw gap restores the comparison to what the author actually
/// wrote before ranking, turning what was a phantom 1-byte win into a
/// genuine tie — and Rust's `min_by_key` breaks a genuine tie by
/// returning the FIRST element, which is document order, i.e. the
/// PRECEDING heading again. So a tie after correction still needs an
/// explicit tiebreak: the boolean carried alongside each gap prefers the
/// heading the anchor PRECEDES (the one it "names" in the reclassification
/// this function performs) whenever the corrected gaps are equal.
fn heading_adjacent_to(
    source: &str,
    def_start: usize,
    def_end: usize,
    raw_headings: &[RawHeading],
) -> Option<usize> {
    raw_headings
        .iter()
        .enumerate()
        .filter_map(|(idx, h)| {
            if def_end <= h.start && source[def_end..h.start].trim().is_empty() {
                // The anchor precedes `h` — measured as written, no
                // correction needed on this side. `anchor_precedes_h` is
                // `true`: `h` is the heading the anchor "names", and wins
                // a corrected-gap tie below.
                Some((idx, h.start - def_end, true))
            } else if h.end <= def_start && source[h.end..def_start].trim().is_empty() {
                // `h` precedes the anchor — `h.end` already ate one
                // newline that never separated the two on the other
                // side's measurement; add it back before comparing.
                Some((idx, def_start - h.end + 1, false))
            } else {
                None
            }
        })
        .min_by_key(|&(_, gap, anchor_precedes_h)| (gap, !anchor_precedes_h))
        .map(|(idx, _, _)| idx)
}

/// Where an inline-form definition's (bold- or FREE-STANDING html-form)
/// prose scope ends: the next heading of ANY level, or the next
/// definition of EITHER other inline form, whichever comes first. An
/// inline definition is a sentence inside a section, not a section of
/// its own — unlike a heading-form definition, whose scope survives a
/// deeper subheading
/// (`nested_headings_find_the_nearest_bracket_kebab_ancestor`), nothing
/// beneath the next heading, and nothing past the next sibling
/// definition, belongs to it. Shared by both `AnchorKind::Bold` and a
/// non-heading-adjacent `AnchorKind::Html` rather than duplicated per
/// form, since the rule itself does not depend on which inline form is
/// asking — a bold definition's scope now closes at the next FREE-STANDING
/// html definition too, and vice versa, symmetrically. A heading-adjacent
/// html anchor never reaches this function at all —
/// `heading_adjacent_to` reclassifies it to `AnchorKind::Heading` before
/// scope computation ever runs.
fn inline_scope_end(
    after: usize,
    raw_headings: &[RawHeading],
    raw_bold_defs: &[RawBoldDef],
    raw_html_defs: &[RawHtmlDef],
    source_len: usize,
) -> usize {
    [
        raw_headings
            .iter()
            .find(|h| h.start > after)
            .map(|h| h.start),
        raw_bold_defs
            .iter()
            .find(|b| b.start > after)
            .map(|b| b.start),
        raw_html_defs
            .iter()
            .find(|h| h.start > after)
            .map(|h| h.start),
    ]
    .into_iter()
    .flatten()
    .min()
    .unwrap_or(source_len)
}

/// The raw, byte-offset-carrying result of walking a document's event
/// stream once — everything [`extract_document`] and [`find_rename_sites`]
/// both need, factored out so the second is built from the SAME
/// recognizer pass as the first rather than a re-derivation of it. This is
/// the concrete meaning of "ranges come from the extractor's own
/// offsets" (the rename dispatch's closed-by-construction ruling):
/// [`find_rename_sites`] never re-parses prose or re-implements a
/// recognizer, it only reads byte spans this same walk already computed.
struct Scan {
    line_index: LineIndex,
    raw_headings: Vec<RawHeading>,
    raw_links: Vec<RawLink>,
    raw_codes: Vec<RawCode>,
    all_code_spans: Vec<RawCode>,
    raw_blocks: Vec<RawBlock>,
    raw_bold_defs: Vec<RawBoldDef>,
    raw_malformed_bold: Vec<RawMalformedBold>,
    raw_html_defs: Vec<RawHtmlDef>,
    raw_malformed_html: Vec<RawMalformedHtml>,
    normative_occurrences: Vec<NormativeOccurrence>,
}

/// Walk `source`'s event stream once, collecting every raw, byte-offset
/// form of heading/bold/html definition, link, code span, and fenced
/// block. `file` is only needed to stamp [`NormativeOccurrence`] (the one
/// output here that already carries a file name rather than a bare byte
/// offset).
fn scan(file: &str, source: &str) -> Scan {
    let line_index = LineIndex::new(source);
    let parser = Parser::new_ext(source, Options::empty()).into_offset_iter();

    let mut raw_headings: Vec<RawHeading> = Vec::new();
    let mut raw_links: Vec<RawLink> = Vec::new();
    let mut raw_codes: Vec<RawCode> = Vec::new();
    let mut all_code_spans: Vec<RawCode> = Vec::new();
    let mut raw_blocks: Vec<RawBlock> = Vec::new();
    let mut raw_bold_defs: Vec<RawBoldDef> = Vec::new();
    let mut raw_malformed_bold: Vec<RawMalformedBold> = Vec::new();
    let mut raw_html_defs: Vec<RawHtmlDef> = Vec::new();
    let mut raw_malformed_html: Vec<RawMalformedHtml> = Vec::new();

    // pulldown-cmark's offset iterator gives Start and End the same full
    // element range, so we capture level/start/end at Start and only
    // accumulate text until End closes it.
    let mut cur_heading: Option<(u8, usize, usize, String)> = None;
    let mut cur_block: Option<(usize, usize, bool, String)> = None;
    // A `**…**` span that opened at line start (byte immediately before
    // `**` is `\n` or file-start) — the first of the bold form's four
    // properties. Non-line-start bold spans are never tracked here at
    // all, which is what keeps a mid-sentence bold run cheap to ignore.
    let mut cur_strong: Option<(usize, usize, String)> = None;
    // A closed line-start `**[id]**` awaiting its definitional
    // punctuation — the fourth property. See `PendingBold`'s own docs for
    // how far this state can carry; ordinary bold text and a mid-sentence
    // citation of a real id (no colon ever follows) both fall through
    // silently, same as before this state grew multi-event.
    let mut pending_bold: Option<PendingBold> = None;
    // An `<a id="…">` open tag awaiting its immediately-adjacent `</a>` —
    // see `PendingHtml`'s own docs.
    let mut pending_html: Option<PendingHtml> = None;
    // Nesting depth, not a bool: a quote can contain a quote. Only its
    // zero/nonzero state matters to the normative-prose scan below.
    let mut blockquote_depth: u32 = 0;
    let mut normative_occurrences: Vec<NormativeOccurrence> = Vec::new();

    for (event, range) in parser {
        if let Some(p) = pending_html.take() {
            let closes = range.start == p.end_of_open
                && matches!(&event, Event::InlineHtml(t) | Event::Html(t) if is_html_close_tag(t, "a"));
            if closes {
                match p.candidate {
                    HtmlCandidate::Definition(id) => raw_html_defs.push(RawHtmlDef {
                        start: p.start,
                        end: range.end,
                        id,
                    }),
                    HtmlCandidate::Malformed(id) => {
                        raw_malformed_html.push(RawMalformedHtml { start: p.start, id });
                    }
                }
            }
            // else: not immediately closed — abandon silently (`PendingHtml`'s
            // doc comment). The current event still needs its own normal
            // handling below (it may itself open a new candidate), so it
            // falls through rather than `continue`-ing.
        }

        if let Some(mut pb) = pending_bold.take() {
            if pb.skip_depth > 0 {
                // Inside an opaque wrapper span whose whole byte range is
                // already accounted for in `pb.cursor` — track balance
                // only, never scan for a colon in here.
                match &event {
                    Event::Start(_) => pb.skip_depth += 1,
                    Event::End(_) => pb.skip_depth -= 1,
                    _ => {}
                }
                pending_bold = Some(pb);
            } else if range.start == pb.cursor {
                match &event {
                    Event::Text(t) => {
                        let budget =
                            MAX_INTERSTITIAL_LEN.saturating_sub(pb.cursor - pb.interstitial_start);
                        match scan_interstitial_chunk(t, budget) {
                            InterstitialStep::ResolvedAt(rel) => match pb.candidate {
                                BoldCandidate::Definition(id) => raw_bold_defs.push(RawBoldDef {
                                    start: pb.start,
                                    end: range.start + rel + 1,
                                    id,
                                }),
                                BoldCandidate::Malformed(id) => {
                                    raw_malformed_bold.push(RawMalformedBold {
                                        start: pb.start,
                                        id,
                                    });
                                }
                            },
                            InterstitialStep::Continue => {
                                pb.cursor = range.end;
                                pending_bold = Some(pb);
                            }
                            InterstitialStep::Failed => {}
                        }
                    }
                    Event::Start(_) => {
                        // A balanced inline wrapper opening exactly where
                        // the interstitial continues (`PendingBold`'s
                        // docs): pre-consume its whole span in one hop.
                        let new_cursor = range.end;
                        if new_cursor - pb.interstitial_start <= MAX_INTERSTITIAL_LEN {
                            pb.cursor = new_cursor;
                            pb.skip_depth = 1;
                            pending_bold = Some(pb);
                        }
                    }
                    // A line break (Soft/HardBreak), inline code, or
                    // anything else: MVP.md §1.1's "immediately followed"
                    // does not survive crossing a line, and no other
                    // event kind is a recognized wrapper — abandon.
                    _ => {}
                }
            }
            // else: a gap between events broke the interstitial's
            // continuity — abandon (already None from `take()`).
        }

        match event {
            Event::Start(Tag::BlockQuote(_)) => blockquote_depth += 1,
            Event::End(TagEnd::BlockQuote(_)) => {
                blockquote_depth = blockquote_depth.saturating_sub(1);
            }
            Event::Start(Tag::Heading { level, .. }) => {
                cur_heading = Some((
                    heading_level_u8(level),
                    range.start,
                    range.end,
                    String::new(),
                ));
            }
            Event::End(TagEnd::Heading(_)) => {
                if let Some((level, start, end, text)) = cur_heading.take() {
                    raw_headings.push(RawHeading {
                        level,
                        start,
                        end,
                        text: text.trim().to_string(),
                    });
                }
            }
            Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(info))) => {
                cur_block = Some((
                    range.start,
                    range.end,
                    info.trim() == "claim",
                    String::new(),
                ));
            }
            Event::End(TagEnd::CodeBlock) => {
                if let Some((start, end, is_claim, yaml)) = cur_block.take() {
                    raw_blocks.push(RawBlock {
                        start,
                        end,
                        is_claim,
                        yaml,
                    });
                }
            }
            Event::Start(Tag::Strong) => {
                let line_start =
                    range.start == 0 || source.as_bytes().get(range.start - 1) == Some(&b'\n');
                cur_strong = line_start.then(|| (range.start, range.end, String::new()));
            }
            Event::End(TagEnd::Strong) => {
                if let Some((start, end, text)) = cur_strong.take() {
                    let trimmed = text.trim();
                    let candidate = if let Some(id) = bracket_kebab_id(trimmed) {
                        Some(BoldCandidate::Definition(id))
                    } else {
                        malformed_bracket_id(trimmed)
                            .map(|id| BoldCandidate::Malformed(id.to_string()))
                    };
                    if let Some(candidate) = candidate {
                        pending_bold = Some(PendingBold {
                            start,
                            interstitial_start: end,
                            cursor: end,
                            candidate,
                            skip_depth: 0,
                        });
                    }
                }
            }
            Event::Start(Tag::Link { dest_url, .. }) => {
                raw_links.push(RawLink {
                    start: range.start,
                    dest: dest_url.into_string(),
                });
            }
            Event::InlineHtml(ref t) | Event::Html(ref t) => {
                // An `<a href="…">` (no `id` attribute) is indistinguishable
                // from prose that never attempted a definition — never a
                // candidate, not even a malformed one, the same silence an
                // ordinary non-line-start bold span gets.
                if html_open_tag_name(t).is_some_and(|name| name.eq_ignore_ascii_case("a"))
                    && let Some(id) = html_attr_value(t, "id")
                {
                    let candidate = if is_kebab_case(id) {
                        HtmlCandidate::Definition(id.to_string())
                    } else {
                        HtmlCandidate::Malformed(id.to_string())
                    };
                    pending_html = Some(PendingHtml {
                        start: range.start,
                        end_of_open: range.end,
                        candidate,
                    });
                }
            }
            Event::Text(t) => {
                if let Some((_, _, _, ref mut text)) = cur_heading {
                    text.push_str(&t);
                }
                if let Some((_, _, ref mut text)) = cur_strong {
                    text.push_str(&t);
                }
                if let Some((_, _, _, ref mut yaml)) = cur_block {
                    yaml.push_str(&t);
                }
                // normative-prose scan: "own voice" is everything but a
                // block quote (any depth) or a fenced code block (`claim`
                // or otherwise — cur_block is set for every fence, so a
                // claim block's own YAML is exempted the same way an
                // ordinary code sample is). A heading counts as own
                // voice like any other text; nothing here exempts it.
                // Inline code spans need no separate exclusion:
                // pulldown-cmark emits them as `Event::Code`, never
                // `Event::Text` — this arm simply never sees them.
                if blockquote_depth == 0 && cur_block.is_none() {
                    normative_occurrences.extend(scan_normative_keywords(
                        file,
                        &t,
                        range.start,
                        &line_index,
                    ));
                }
            }
            Event::Code(t) => {
                // Every inline code span, corpus-wide and regardless of
                // which other role the same span plays below, is a
                // candidate `unreachable-reference` target
                // (`gitignore::path_shaped` decides downstream whether its
                // content actually looks like a path) — a `.ledger/…`
                // citation written as `` `.ledger/foo.md` `` is just as
                // unreachable for a reader inside a heading as it is in
                // ordinary prose, and this collection is unscoped by
                // design: unlike `raw_codes`/`prose_code` below, there is
                // no claim-window or own-voice question here, only "does
                // a reader of this file see a pointer they cannot follow."
                all_code_spans.push(RawCode {
                    start: range.start,
                    text: t.to_string(),
                });

                // Inline code spans occur only in inline (heading/prose)
                // context; a fenced block's content is Text, never Code.
                // A heading's own code span feeds only the heading's own
                // text, never `raw_codes` (`RawCode`'s doc comment): a
                // literal that appears solely inside a sub-heading within
                // a claim's scope is not prose mentioning it, so it must
                // not silence `absent-marker-stale` for that literal.
                if let Some((_, _, _, ref mut text)) = cur_heading {
                    text.push_str(&t);
                } else {
                    raw_codes.push(RawCode {
                        start: range.start,
                        text: t.to_string(),
                    });
                }
            }
            Event::SoftBreak | Event::HardBreak => {
                if let Some((_, _, _, ref mut text)) = cur_heading {
                    text.push(' ');
                }
            }
            _ => {}
        }
    }

    Scan {
        line_index,
        raw_headings,
        raw_links,
        raw_codes,
        all_code_spans,
        raw_blocks,
        raw_bold_defs,
        raw_malformed_bold,
        raw_html_defs,
        raw_malformed_html,
        normative_occurrences,
    }
}

/// Parse one markdown document and extract its claim blocks, per MVP.md
/// §1.1. `file` is the corpus-relative path recorded on each claim.
pub fn extract_document(file: &str, source: &str) -> ExtractResult {
    let Scan {
        line_index,
        raw_headings,
        raw_links,
        raw_codes,
        all_code_spans,
        raw_blocks,
        raw_bold_defs,
        raw_malformed_bold,
        raw_html_defs,
        raw_malformed_html,
        normative_occurrences,
    } = scan(file, source);

    // Real GitHub-style slugs, deduplicated in document order
    // (`model::assign_heading_slugs`) — GitHub's own dedup counter resets
    // per document too, so this must run once per `extract_document` call
    // over that document's own headings in reading order, not corpus-wide.
    let slugs = assign_heading_slugs(raw_headings.iter().map(|h| h.text.as_str()));
    let headings: Vec<Heading> = raw_headings
        .iter()
        .zip(slugs)
        .map(|(h, slug)| Heading {
            level: h.level,
            text: h.text.clone(),
            line: line_index.line_of(h.start),
            slug,
        })
        .collect();

    // One position-ordered stream of every recognized id definition,
    // heading-form and bold-form together (MVP.md §1.1, generalized): a
    // claim block's owner is the nearest preceding definition regardless
    // of which form it is, exactly the existing "deeper heading wins"
    // rule extended from one shape to two.
    let mut id_anchors: Vec<IdAnchor> = raw_headings
        .iter()
        .enumerate()
        .filter_map(|(i, h)| {
            heading_bracket_kebab_id(&h.text).map(|id| IdAnchor {
                start: h.start,
                id,
                kind: AnchorKind::Heading(i),
            })
        })
        .chain(raw_bold_defs.iter().enumerate().map(|(i, b)| IdAnchor {
            start: b.start,
            id: b.id.clone(),
            kind: AnchorKind::Bold(i),
        }))
        .chain(raw_html_defs.iter().enumerate().map(|(i, h)| {
            match heading_adjacent_to(source, h.start, h.end, &raw_headings) {
                // Heading-adjacent: this anchor names the SECTION, not a
                // sentence — reclassified to `Heading`, inheriting that
                // heading's level and extent exactly as if the heading
                // itself had carried a bracket-kebab id (`heading_adjacent_to`'s
                // own doc comment states why this must not fall through
                // to `inline_scope_end`). `start` is the earlier of the
                // two markers, so the pair's ownership position is
                // unaffected by which order the author wrote them in.
                Some(heading_idx) => IdAnchor {
                    start: h.start.min(raw_headings[heading_idx].start),
                    id: h.id.clone(),
                    kind: AnchorKind::Heading(heading_idx),
                },
                None => IdAnchor {
                    start: h.start,
                    id: h.id.clone(),
                    kind: AnchorKind::Html(i),
                },
            }
        }))
        .collect();
    id_anchors.sort_by_key(|a| a.start);

    let mut claims = Vec::new();
    let mut orphan_claims = Vec::new();
    let mut claimed_anchor_starts: std::collections::HashSet<usize> =
        std::collections::HashSet::new();

    for block in raw_blocks.iter().filter(|b| b.is_claim) {
        let block_line = line_index.line_of(block.start);

        let found = id_anchors
            .iter()
            .take_while(|a| a.start < block.start)
            .last();

        let Some(anchor) = found else {
            orphan_claims.push(OrphanClaim {
                file: file.to_string(),
                line: block_line,
            });
            continue;
        };

        // Prose scope (C5, §3), per form:
        // - heading-form: from the id heading to the next heading of the
        //   same or higher level (unchanged from before this dispatch).
        // - bold-form: from the end of the matched punctuation to the
        //   next definition of EITHER form, or the next heading of ANY
        //   level — a bold-form definition is inline prose, not a
        //   section, so nothing beneath a subheading and nothing past
        //   the next definition belongs to it. Human-authored analogue:
        //   the paragraph(s) discussing this claim, up to whatever comes
        //   next.
        let (id, heading_line, scope_start, scope_end) = match anchor.kind {
            AnchorKind::Heading(idx) => {
                let heading = &raw_headings[idx];
                let scope_start = heading.end;
                let scope_end = raw_headings[idx + 1..]
                    .iter()
                    .find(|h2| h2.level <= heading.level)
                    .map(|h2| h2.start)
                    .unwrap_or(source.len());
                (
                    anchor.id.clone(),
                    line_index.line_of(heading.start),
                    scope_start,
                    scope_end,
                )
            }
            AnchorKind::Bold(idx) => {
                let bold = &raw_bold_defs[idx];
                let scope_end = inline_scope_end(
                    bold.end,
                    &raw_headings,
                    &raw_bold_defs,
                    &raw_html_defs,
                    source.len(),
                );
                (
                    anchor.id.clone(),
                    line_index.line_of(bold.start),
                    bold.end,
                    scope_end,
                )
            }
            AnchorKind::Html(idx) => {
                let html = &raw_html_defs[idx];
                let scope_end = inline_scope_end(
                    html.end,
                    &raw_headings,
                    &raw_bold_defs,
                    &raw_html_defs,
                    source.len(),
                );
                (
                    anchor.id.clone(),
                    line_index.line_of(html.start),
                    html.end,
                    scope_end,
                )
            }
        };

        claimed_anchor_starts.insert(anchor.start);

        let prose_links: Vec<String> = raw_links
            .iter()
            .filter(|l| l.start >= scope_start && l.start < scope_end)
            .filter(|l| !(l.start >= block.start && l.start < block.end))
            .filter(|l| !is_external(&l.dest))
            .map(|l| l.dest.clone())
            .collect();

        let prose_code: Vec<String> = raw_codes
            .iter()
            .filter(|c| c.start >= scope_start && c.start < scope_end)
            .filter(|c| !(c.start >= block.start && c.start < block.end))
            .map(|c| c.text.clone())
            .collect();

        let raw = parse_raw_claim_block(&block.yaml);
        let depends = raw.depends.iter().map(|c| CiteRef::parse(c)).collect();
        let because = raw.because.iter().map(|c| CiteRef::parse(c)).collect();

        claims.push(Claim {
            id,
            file: file.to_string(),
            heading_line,
            block_line,
            raw,
            depends,
            because,
            prose_links,
            prose_code,
        });
    }

    // Coverage count (the dispatch's second deliverable): a recognized
    // definition — either form — that no claim block ever adopted as its
    // nearest preceding anchor. Reported, never failed on (checks.rs).
    let unregistered_definitions: Vec<UnregisteredDefinition> = id_anchors
        .iter()
        .filter(|a| !claimed_anchor_starts.contains(&a.start))
        .map(|a| UnregisteredDefinition {
            file: file.to_string(),
            line: line_index.line_of(a.start),
            id: a.id.clone(),
        })
        .collect();

    // `malformed-id` (`.ledger/2026-08-04-malformed-ids-are-silently-invisible.md`):
    // a bracketed token in definition position that fails the id grammar,
    // heading-form and bold-form together — position-ordered the same way
    // `id_anchors` is, for a deterministic, reading-order diagnostic list.
    let mut malformed_ids: Vec<(usize, MalformedId)> = raw_headings
        .iter()
        .filter_map(|h| {
            malformed_bracket_id(&h.text).map(|inner| {
                (
                    h.start,
                    MalformedId {
                        file: file.to_string(),
                        line: line_index.line_of(h.start),
                        id: inner.to_string(),
                    },
                )
            })
        })
        .chain(raw_malformed_bold.iter().map(|b| {
            (
                b.start,
                MalformedId {
                    file: file.to_string(),
                    line: line_index.line_of(b.start),
                    id: b.id.clone(),
                },
            )
        }))
        .chain(raw_malformed_html.iter().map(|h| {
            (
                h.start,
                MalformedId {
                    file: file.to_string(),
                    line: line_index.line_of(h.start),
                    id: h.id.clone(),
                },
            )
        }))
        .collect();
    malformed_ids.sort_by_key(|(start, _)| *start);
    let malformed_ids = malformed_ids.into_iter().map(|(_, m)| m).collect();

    // The whole document's link surface (`unreachable-reference`), not
    // scoped to any claim — a document with no claims at all still has
    // links, and `prose_links` above only ever sees the ones inside a
    // claim's own C5 window.
    let links: Vec<DocumentLink> = raw_links
        .iter()
        .filter(|l| !is_external(&l.dest))
        .map(|l| DocumentLink {
            file: file.to_string(),
            line: line_index.line_of(l.start),
            dest: l.dest.clone(),
        })
        .collect();

    // Every inline code span, corpus-wide, as an `unreachable-reference`
    // candidate — `all_code_spans` above, not `raw_codes` (which is
    // scoped to claim windows for `absent-marker-stale`). Feeds a
    // SEPARATE field from `links`, never `dangling-reference`/C5: an
    // inline code span is not a link, and treating one as a resolvable
    // reference for those checks would fire on every incidental
    // `` `docs/x.md` `` mention that was never meant as a citation.
    // `gitignore::path_shaped`/`find_unreachable_references` still decide
    // whether any given span's content actually looks like a path and
    // whether it resolves to something gitignored.
    let code_references: Vec<DocumentLink> = all_code_spans
        .iter()
        .filter(|c| !is_external(&c.text))
        .map(|c| DocumentLink {
            file: file.to_string(),
            line: line_index.line_of(c.start),
            dest: c.text.clone(),
        })
        .collect();

    ExtractResult {
        headings,
        claims,
        orphan_claims,
        normative_occurrences,
        unregistered_definitions,
        malformed_ids,
        links,
        code_references,
    }
}

// --- rename: byte-exact site location -----------------------------------
//
// `docket rename`'s writer (`rename.rs`) is closed by construction: its
// only primitive is "replace byte range [s,e) with a rendering of id X",
// where every range must come from here. Because a claim id is a plain
// kebab-case token, "a rendering of id X" is always just `X`'s own bytes —
// no bracket, quote, or link syntax ever needs re-synthesizing, only
// located and substituted in place. That is what turns "every reference
// to the id" into a byte-span-finding problem rather than a
// document-rewriting one.

/// One byte-exact site `rename.rs` may substitute another id's bytes
/// into. Always spans exactly `target_id`'s own bytes — never a bracket,
/// quote, comma, or any surrounding syntax — which `rename.rs`'s `Edit`
/// constructor re-verifies (`source[start..end] == target_id`) before
/// building an edit from it. That verification is what makes the writer
/// closed by construction: even if this function's site-finding logic had
/// a defect, an edit could never touch a byte outside a confirmed
/// occurrence of the exact id being renamed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenameSite {
    pub start: usize,
    pub end: usize,
    pub kind: RenameSiteKind,
}

/// What kind of site a [`RenameSite`] is — carried for `rename.rs`'s plan
/// output and diagnostics, not consulted by the substitution itself
/// (every kind substitutes the same way: replace the span with the new
/// id's bytes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenameSiteKind {
    /// The id's own definition — heading, bold, or html form (MVP.md
    /// §1.1).
    Definition,
    /// A `depends:` array entry naming the claim id bare (no `#`
    /// document-anchor suffix — those name a section, not this claim).
    Depends,
    /// A `because:` array entry, same shape as `Depends`.
    Because,
    /// A markdown link's href: the bare id as the whole destination, or
    /// the fragment after `#` in an anchor-form destination (MVP.md
    /// §1.3, "Which link forms satisfy a declaration").
    ProseLink,
}

/// Every byte span in `source` where `target_id` appears in a position
/// this project's grammar recognizes as naming that claim: its own
/// definition (any of the three forms), every bare `depends`/`because`
/// entry naming it, and every prose link (bare-id or anchor-form href)
/// naming it.
///
/// Built from the SAME raw recognizer pass [`extract_document`] uses
/// ([`scan`]) — a site this function returns is, by construction, a site
/// the real extractor already recognizes as belonging to `target_id`, not
/// a second, independent text search that could disagree with it.
/// Deliberately narrower than "every occurrence of the substring
/// `target_id`": an id appearing inside ordinary prose, a code span, or as
/// part of a longer word is never a site, because `extract_document`
/// itself would never have parsed it as a definition or a reference.
pub fn find_rename_sites(source: &str, target_id: &str) -> Vec<RenameSite> {
    let scanned = scan("", source);
    let mut sites = Vec::new();

    for h in scanned
        .raw_headings
        .iter()
        .filter(|h| heading_bracket_kebab_id(&h.text).as_deref() == Some(target_id))
    {
        if let Some((start, end)) = find_bracket_span(source, h.start, h.end, target_id) {
            sites.push(RenameSite {
                start,
                end,
                kind: RenameSiteKind::Definition,
            });
        }
    }

    for b in scanned.raw_bold_defs.iter().filter(|b| b.id == target_id) {
        if let Some((start, end)) = find_bracket_span(source, b.start, b.end, target_id) {
            sites.push(RenameSite {
                start,
                end,
                kind: RenameSiteKind::Definition,
            });
        }
    }

    for h in scanned.raw_html_defs.iter().filter(|h| h.id == target_id) {
        if let Some((start, end)) = find_html_id_attr_span(source, h.start, h.end, target_id) {
            sites.push(RenameSite {
                start,
                end,
                kind: RenameSiteKind::Definition,
            });
        }
    }

    for block in scanned.raw_blocks.iter().filter(|b| b.is_claim) {
        for (field, kind) in [
            ("depends", RenameSiteKind::Depends),
            ("because", RenameSiteKind::Because),
        ] {
            for (start, end) in
                find_yaml_ref_spans(source, block.start, block.end, field, target_id)
            {
                sites.push(RenameSite { start, end, kind });
            }
        }
    }

    for l in &scanned.raw_links {
        let matches_target =
            l.dest == target_id || l.dest.rsplit_once('#').is_some_and(|(_, f)| f == target_id);
        if !matches_target {
            continue;
        }
        if let Some((start, end)) = find_link_id_span(source, l.start, &l.dest, target_id) {
            sites.push(RenameSite {
                start,
                end,
                kind: RenameSiteKind::ProseLink,
            });
        }
    }

    sites.sort_by_key(|s| s.start);
    sites
}

/// Locate the literal `[id]` bracket span inside `source[region_start..region_end]`
/// — the definition's own bracket pair, for the heading and bold forms — and
/// return the byte range of just `id`'s own bytes (not the brackets).
/// `region_start`/`region_end` are already a recognized definition's own
/// span (a `RawHeading` or `RawBoldDef`), so a literal search inside it is
/// safe: MVP.md §1.1 requires the heading's whole trimmed text (or the
/// bold span's whole content) to equal `[id]` exactly, so nothing else in
/// that span could produce a second, spurious match.
fn find_bracket_span(
    source: &str,
    region_start: usize,
    region_end: usize,
    id: &str,
) -> Option<(usize, usize)> {
    let region = source.get(region_start..region_end)?;
    let needle = format!("[{id}]");
    let rel = region.find(&needle)?;
    let start = region_start + rel + 1;
    Some((start, start + id.len()))
}

/// Locate the literal `id="…"`/`id='…'` attribute value's byte span
/// inside `source[region_start..region_end]` — an html-form definition's
/// own `<a id="…">` span (a `RawHtmlDef`). Requires a token boundary
/// (start-of-region or preceding whitespace) immediately before `id=`, so
/// a coincidental `data-id="…"` bearing the identical value is never
/// mistaken for the `id` attribute itself — the same false-positive floor
/// [`html_attr_value`] already enforces via `split_whitespace`, restated
/// here because this function additionally needs the value's own byte
/// offset, which a value-only lookup can't provide.
fn find_html_id_attr_span(
    source: &str,
    region_start: usize,
    region_end: usize,
    id: &str,
) -> Option<(usize, usize)> {
    let region = source.get(region_start..region_end)?;
    for quote in ['"', '\''] {
        let needle = format!("id={quote}{id}{quote}");
        let mut search_from = 0;
        while let Some(rel) = region[search_from..].find(&needle) {
            let abs_rel = search_from + rel;
            let boundary_ok = abs_rel == 0
                || region.as_bytes()[abs_rel - 1].is_ascii_whitespace()
                || region.as_bytes()[abs_rel - 1] == b'<';
            if boundary_ok {
                let start = region_start + abs_rel + 3 + 1; // "id=".len() + the quote byte
                return Some((start, start + id.len()));
            }
            search_from = abs_rel + 1;
        }
    }
    None
}

/// The byte offset of the start of every line in `s` (0-indexed within
/// `s`, not a 1-indexed [`Line`]) — [`find_yaml_ref_spans`]'s own
/// line-at-a-time scan over an already-isolated claim block's text, the
/// same class of hand-rolled tokenizing [`bracket_kebab_id`] and
/// [`ascii_words`] already do rather than a regex over document
/// structure.
fn line_starts(s: &str) -> Vec<usize> {
    std::iter::once(0)
        .chain(s.match_indices('\n').map(|(i, _)| i + 1))
        .filter(|&i| i < s.len())
        .collect()
}

/// Locate every occurrence of `target_id` as a bare `depends`/`because`
/// array entry inside a claim block's own source span
/// (`source[block_start..block_end]`, a `RawBlock`'s span) — one lexical
/// scan per `field` ("depends" or "because"), restricted to a line whose
/// trimmed text starts with `<field>:`, matching the flow-sequence shape
/// every real claim block in this corpus already uses
/// (`field: [a, b, c]`). A `depends`/`because` value spanning more than
/// one YAML line is out of this scan's scope — not a shape MVP.md's own
/// examples or this project's own corpus ever produce.
fn find_yaml_ref_spans(
    source: &str,
    block_start: usize,
    block_end: usize,
    field: &str,
    target_id: &str,
) -> Vec<(usize, usize)> {
    let Some(region) = source.get(block_start..block_end) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for line_start in line_starts(region) {
        let rest_of_region = &region[line_start..];
        let line = rest_of_region.split('\n').next().unwrap_or(rest_of_region);
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();
        let Some(after_field) = trimmed.strip_prefix(field) else {
            continue;
        };
        let Some(after_colon) = after_field.strip_prefix(':') else {
            continue;
        };
        let Some(open_rel) = after_colon.find('[') else {
            continue;
        };
        let Some(close_rel) = after_colon[open_rel..].find(']') else {
            continue;
        };
        let seq = &after_colon[open_rel + 1..open_rel + close_rel];
        let seq_abs_start = block_start + line_start + indent + field.len() + 1 + open_rel + 1;

        let mut offset = 0usize;
        for entry in seq.split(',') {
            let entry_trimmed = entry.trim();
            let leading = entry.len() - entry.trim_start().len();
            if entry_trimmed == target_id {
                let start = seq_abs_start + offset + leading;
                out.push((start, start + target_id.len()));
            }
            offset += entry.len() + 1; // +1 accounts for the consumed comma
        }
    }
    out
}

/// Locate `dest`'s own id-bearing byte span within the markdown link
/// starting at `link_start` (a `RawLink`'s `start`, the link's opening
/// `[`) — either the whole destination (bare-id form) or the fragment
/// after its last `#` (anchor form). Requires the raw source text between
/// `](` and the next `)` to equal `dest` byte-for-byte before trusting the
/// span: `dest` is pulldown-cmark's already-decoded `dest_url`, which can
/// differ from the raw source text for an entity- or percent-escaped
/// href, and this function refuses to guess in that case rather than risk
/// substituting into the wrong bytes — the staged invariant check
/// (`rename.rs`) then reports the resulting divergence instead of this
/// function silently mis-locating one.
fn find_link_id_span(
    source: &str,
    link_start: usize,
    dest: &str,
    target_id: &str,
) -> Option<(usize, usize)> {
    // Real inline link syntax never spans more than a modest window
    // between its opening `[` and its closing `)`; bounding the search
    // keeps this a local scan rather than an accidental corpus-wide one.
    let window_end = (link_start + 4096).min(source.len());
    let window = source.get(link_start..window_end)?;
    let paren_rel = window.find("](")?;
    let dest_start_rel = paren_rel + 2;
    let close_rel = window.get(dest_start_rel..)?.find(')')?;
    let raw_dest = &window[dest_start_rel..dest_start_rel + close_rel];
    if raw_dest != dest {
        return None;
    }
    let dest_abs_start = link_start + dest_start_rel;
    if dest == target_id {
        return Some((dest_abs_start, dest_abs_start + target_id.len()));
    }
    let hash_rel = dest.rfind('#')?;
    if &dest[hash_rel + 1..] == target_id {
        let start = dest_abs_start + hash_rel + 1;
        return Some((start, start + target_id.len()));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(res: &ExtractResult) -> Vec<&str> {
        res.claims.iter().map(|c| c.id.as_str()).collect()
    }

    #[test]
    fn extracts_a_simple_claim() {
        let src = "### [lock-groundness]\n\nEvery lock value MUST be ground.\n\n```claim\nkind: constraint\nevaluator: property-test\ndepends: [docs/models/composition-model#6]\n```\n";
        let res = extract_document("docs/specs/lock.md", src);
        assert_eq!(ids(&res), vec!["lock-groundness"]);
        let claim = &res.claims[0];
        assert_eq!(claim.raw.kind.as_deref(), Some("constraint"));
        assert_eq!(claim.raw.evaluator.as_deref(), Some("property-test"));
        assert_eq!(
            claim.depends,
            vec![CiteRef::DocAnchor {
                path: "docs/models/composition-model".into(),
                anchor: "6".into()
            }]
        );
        assert!(claim.because.is_empty());
        assert!(res.orphan_claims.is_empty());
    }

    #[test]
    fn extracts_a_because_entry_separately_from_depends() {
        let src = "### [x]\n\n```claim\nkind: constraint\nevaluator: test\ndepends: [d#1]\nbecause: [b#2]\n```\n";
        let res = extract_document("docs/specs/x.md", src);
        let claim = &res.claims[0];
        assert_eq!(
            claim.depends,
            vec![CiteRef::DocAnchor {
                path: "d".into(),
                anchor: "1".into()
            }]
        );
        assert_eq!(
            claim.because,
            vec![CiteRef::DocAnchor {
                path: "b".into(),
                anchor: "2".into()
            }]
        );
    }

    #[test]
    fn claim_block_with_no_preceding_id_heading_is_orphan() {
        let src = "## Notes\n\n```claim\nkind: constraint\n```\n";
        let res = extract_document("docs/specs/x.md", src);
        assert!(res.claims.is_empty());
        assert_eq!(res.orphan_claims.len(), 1);
        assert_eq!(res.orphan_claims[0].file, "docs/specs/x.md");
    }

    #[test]
    fn claim_block_at_top_of_file_with_no_heading_at_all_is_orphan() {
        let src = "```claim\nkind: constraint\n```\n";
        let res = extract_document("docs/specs/x.md", src);
        assert!(res.claims.is_empty());
        assert_eq!(res.orphan_claims.len(), 1);
    }

    #[test]
    fn nested_headings_find_the_nearest_bracket_kebab_ancestor() {
        // The id heading is level 2; a level-4 non-id subheading
        // intervenes before the claim block. The nearest preceding
        // bracket-kebab heading is still [outer-id] (level 4 isn't a
        // bracket-kebab heading, so the search skips over it).
        let src = "## [outer-id]\n\nsome prose\n\n#### Notes\n\n```claim\nkind: requirement\n```\n";
        let res = extract_document("docs/architecture/a.md", src);
        assert_eq!(ids(&res), vec!["outer-id"]);
    }

    #[test]
    fn a_suffixed_retired_heading_registers_as_its_own_claim() {
        // MVP.md §1.1's heading-form id is the bracket itself; a trailing
        // annotation like "— RETIRED <date>" is not part of the id and
        // must not blind the resolver to it. Before the fix,
        // `bracket_kebab_id` required the WHOLE heading text to be
        // exactly `[id]`, so `[b] — RETIRED 2026-08-23` was invisible to
        // id_anchors — its own claim block then attributed to the
        // nearest PRECEDING anchor `[a]`, producing a second Claim with
        // id "a" whose `evaluator: none` clobbers the live claim's
        // `evaluator: test` in the final index (checks.rs builds the
        // index with `BTreeMap::insert`, last write wins).
        let src = "### [a]\n\n```claim\nkind: requirement\nevaluator: test\n```\n\n### [b] — RETIRED 2026-08-23\n\n```claim\nkind: requirement\nevaluator: none\n```\n";
        let res = extract_document("docs/specs/x.md", src);
        assert_eq!(ids(&res), vec!["a", "b"]);
        assert_eq!(res.claims[0].raw.evaluator.as_deref(), Some("test"));
        assert_eq!(res.claims[1].raw.evaluator.as_deref(), Some("none"));
    }

    #[test]
    fn deeper_id_heading_wins_over_a_shallower_one() {
        let src = "## [outer-id]\n\n### [inner-id]\n\n```claim\nkind: requirement\n```\n";
        let res = extract_document("docs/architecture/a.md", src);
        assert_eq!(ids(&res), vec!["inner-id"]);
    }

    #[test]
    fn a_claim_fence_nested_inside_a_wider_fence_is_not_extracted() {
        // This is the exact shape README.md and MVP.md themselves use to
        // show the claim-block syntax: a 4-backtick markdown fence
        // wrapping an example 3-backtick ```claim fence. CommonMark
        // parses the outer fence's content as literal text, so a
        // regex-free, tree-walking extractor must not pick up the inner
        // fence as a real claim block.
        let src = "### [x]\n\n````markdown\n```claim\nkind: constraint\n```\n````\n\n```claim\nkind: invariant\nevaluator: test\n```\n";
        let res = extract_document("docs/specs/x.md", src);
        assert_eq!(ids(&res), vec!["x"]);
        assert_eq!(res.claims[0].raw.kind.as_deref(), Some("invariant"));
    }

    #[test]
    fn prose_link_outside_the_corpus_is_captured_as_a_raw_target_not_dropped_as_external() {
        // C4/C5 decide corpus-membership later (checks.rs); the
        // extractor's only job is to drop links that are unambiguously
        // external (a URL scheme) and pass everything else through.
        let src = "### [x]\n\nSee [unrelated](../not-in-corpus.md) and [the web](https://example.com).\n\n```claim\nkind: constraint\n```\n";
        let res = extract_document("docs/specs/x.md", src);
        assert_eq!(res.claims[0].prose_links, vec!["../not-in-corpus.md"]);
    }

    #[test]
    fn prose_link_scope_stops_at_the_next_same_level_heading() {
        let src = "### [a]\n\n[link-a](target-a)\n\n### [b]\n\n[link-b](target-b)\n\n```claim\nkind: constraint\n```\n";
        let res = extract_document("docs/specs/x.md", src);
        let claim_b = res.claims.iter().find(|c| c.id == "b").unwrap();
        assert_eq!(claim_b.prose_links, vec!["target-b"]);
    }

    #[test]
    fn prose_code_captures_inline_code_spans_in_the_claims_scope() {
        // The `absent-marker-stale` check's adjacency signal
        // (src/absence.rs): a claim's own prose can mark the literal an
        // absence marker names, e.g. "There is no `Retry-After` header."
        let src = "### [x]\n\nThere is no `Retry-After` header on any response.\n\n```claim\nkind: constraint\nevaluator: absent\n```\n";
        let res = extract_document("docs/specs/x.md", src);
        assert_eq!(res.claims[0].prose_code, vec!["Retry-After"]);
    }

    #[test]
    fn prose_code_scope_stops_at_the_next_same_level_heading() {
        let src = "### [a]\n\n`code-a`\n\n### [b]\n\n`code-b`\n\n```claim\nkind: constraint\n```\n";
        let res = extract_document("docs/specs/x.md", src);
        let claim_b = res.claims.iter().find(|c| c.id == "b").unwrap();
        assert_eq!(claim_b.prose_code, vec!["code-b"]);
    }

    #[test]
    fn prose_code_excludes_a_code_span_living_inside_a_sub_heading() {
        // A sub-heading sits within its parent claim's scope (only a
        // same-or-higher-level heading ends it), but a code span inside
        // the sub-heading's own text is not prose *about* the claim —
        // it must not silence `absent-marker-stale` for a literal that
        // never actually appears in the claim's prose body.
        let src = "### [x]\n\nThe old note about it is gone now.\n\n#### `Retry-After` (historical)\n\nSome detail.\n\n```claim\nkind: constraint\nevaluator: absent\n```\n";
        let res = extract_document("docs/specs/x.md", src);
        assert!(
            res.claims[0].prose_code.is_empty(),
            "{:?}",
            res.claims[0].prose_code
        );
    }

    #[test]
    fn unknown_field_is_preserved_raw_for_the_nickel_contract_to_reject() {
        let src = "### [x]\n\n```claim\nkind: constraint\ntypo_field: oops\n```\n";
        let res = extract_document("docs/specs/x.md", src);
        assert!(res.claims[0].raw.yaml.contains("typo_field"));
    }

    #[test]
    fn line_numbers_point_at_the_heading_and_the_fence() {
        let src = "line1\n\n### [x]\nline4\n\n```claim\nkind: constraint\n```\n";
        let res = extract_document("docs/specs/x.md", src);
        let claim = &res.claims[0];
        assert_eq!(claim.heading_line, Line(3));
        assert_eq!(claim.block_line, Line(6));
    }

    #[test]
    fn headings_are_extracted_ready_for_anchor_matching() {
        // model::anchor_matches operates directly on Heading.text; this
        // just confirms extraction hands it clean, unprefixed text.
        let src = "## 6. The fact-set: the substrate's only state\n";
        let res = extract_document("docs/models/composition-model.md", src);
        assert_eq!(
            res.headings[0].text,
            "6. The fact-set: the substrate's only state"
        );
        assert!(crate::model::anchor_matches(&res.headings[0].text, "6"));
        assert!(!crate::model::anchor_matches(&res.headings[0].text, "60"));
    }

    #[test]
    fn extracted_headings_carry_a_real_github_slug() {
        // The exact heading composition-model.md:506 carries, and the
        // exact slug the real corpus's own links already resolve against
        // (adr/0009-atom-composition-plane.md's
        // `composition-model.md#6-the-fact-set-the-substrates-only-state`)
        // — the apostrophe in "substrate's" drops out entirely rather
        // than becoming a separator, same as the slash/period cases
        // `model::tests` pins on the pure function directly.
        let src = "## 6. The fact-set: the substrate's only state\n";
        let res = extract_document("docs/models/composition-model.md", src);
        assert_eq!(
            res.headings[0].slug,
            "6-the-fact-set-the-substrates-only-state"
        );
    }

    #[test]
    fn repeated_headings_in_one_document_get_deduplicated_slugs() {
        let src = "## Overview\n\ntext\n\n## Overview\n\nmore text\n";
        let res = extract_document("docs/guides/a.md", src);
        assert_eq!(res.headings.len(), 2);
        assert_eq!(res.headings[0].slug, "overview");
        assert_eq!(res.headings[1].slug, "overview-1");
    }

    #[test]
    fn an_inline_code_span_in_prose_becomes_a_code_reference_candidate() {
        // `gitignore::path_shaped`/`find_unreachable_references` decide
        // downstream whether the content actually looks like a path;
        // extraction's only job is capturing every span.
        let src = "See `.ledger/2026-01-01-notes.md` for background.\n";
        let res = extract_document("docs/guides/a.md", src);
        assert_eq!(res.code_references.len(), 1);
        assert_eq!(res.code_references[0].dest, ".ledger/2026-01-01-notes.md");
        assert_eq!(res.code_references[0].line, Line(1));
    }

    #[test]
    fn a_code_span_inside_a_heading_still_becomes_a_code_reference() {
        // Deliberately UNSCOPED, unlike `prose_code`/`raw_codes`: a
        // reader sees a heading's own text too, so a citation written
        // there is just as unreachable as one in ordinary prose.
        let src = "## See `.ledger/2026-01-01-notes.md`\n";
        let res = extract_document("docs/guides/a.md", src);
        assert_eq!(res.code_references.len(), 1);
        assert_eq!(res.code_references[0].dest, ".ledger/2026-01-01-notes.md");
    }

    #[test]
    fn an_external_url_inline_code_span_is_not_a_code_reference() {
        let src = "See `https://example.com/notes` for background.\n";
        let res = extract_document("docs/guides/a.md", src);
        assert!(res.code_references.is_empty());
    }

    #[test]
    fn an_ordinary_non_path_inline_code_span_is_still_captured_here() {
        // Filtering by shape is `gitignore::path_shaped`'s job; this
        // extraction layer captures every span unconditionally.
        let src = "Run `cargo test` first.\n";
        let res = extract_document("docs/guides/a.md", src);
        assert_eq!(res.code_references.len(), 1);
        assert_eq!(res.code_references[0].dest, "cargo test");
    }

    #[test]
    fn bracket_kebab_id_rejects_non_kebab_and_non_bracket_text() {
        assert_eq!(
            bracket_kebab_id("[lock-groundness]"),
            Some("lock-groundness".to_string())
        );
        assert_eq!(bracket_kebab_id("[Lock-Groundness]"), None);
        assert_eq!(bracket_kebab_id("[lock_groundness]"), None);
        assert_eq!(bracket_kebab_id("lock-groundness"), None);
        assert_eq!(bracket_kebab_id("[]"), None);
    }

    // --- normative-prose scan --------------------------------------------

    fn keywords(res: &ExtractResult) -> Vec<&str> {
        res.normative_occurrences
            .iter()
            .map(|o| o.keyword)
            .collect()
    }

    #[test]
    fn a_bare_own_voice_keyword_is_captured_with_its_line() {
        let src = "# Decision\n\nThis MUST be treated as final.\n";
        let res = extract_document("docs/adr/x.md", src);
        assert_eq!(keywords(&res), vec!["MUST"]);
        assert_eq!(res.normative_occurrences[0].file, "docs/adr/x.md");
        assert_eq!(res.normative_occurrences[0].line, Line(3));
    }

    #[test]
    fn lowercase_or_mixed_case_is_not_a_keyword() {
        // Capitalisation is the only signal (dispatch, "Detection scope");
        // ordinary English "must"/"Must" must not fire.
        let src = "# Decision\n\nWe must, and we really Must, keep going.\n";
        let res = extract_document("docs/adr/x.md", src);
        assert!(keywords(&res).is_empty());
    }

    #[test]
    fn must_not_is_one_compound_occurrence_not_two() {
        let src = "# Decision\n\nThis MUST NOT be reopened.\n";
        let res = extract_document("docs/adr/x.md", src);
        assert_eq!(keywords(&res), vec!["MUST NOT"]);
    }

    #[test]
    fn should_not_and_shall_not_also_compound() {
        let src = "# Decision\n\nA SHOULD NOT b, and c SHALL NOT d.\n";
        let res = extract_document("docs/adr/x.md", src);
        assert_eq!(keywords(&res), vec!["SHOULD NOT", "SHALL NOT"]);
    }

    #[test]
    fn a_negatable_keyword_without_a_following_not_reports_standalone() {
        let src = "# Decision\n\nThis MUST hold, and that is final.\n";
        let res = extract_document("docs/adr/x.md", src);
        assert_eq!(keywords(&res), vec!["MUST"]);
    }

    #[test]
    fn every_rfc2119_keyword_is_detected() {
        let src = "# D\n\nMUST, MUST NOT, SHOULD, SHOULD NOT, SHALL, SHALL NOT, REQUIRED, RECOMMENDED, MAY, OPTIONAL.\n";
        let res = extract_document("docs/adr/x.md", src);
        assert_eq!(
            keywords(&res),
            vec![
                "MUST",
                "MUST NOT",
                "SHOULD",
                "SHOULD NOT",
                "SHALL",
                "SHALL NOT",
                "REQUIRED",
                "RECOMMENDED",
                "MAY",
                "OPTIONAL",
            ]
        );
    }

    #[test]
    fn a_keyword_inside_a_block_quote_is_exempt_at_any_depth() {
        let src =
            "# Decision\n\n> The proposal said it MUST retry.\n>\n> > Nested: it also MUST log.\n";
        let res = extract_document("docs/adr/x.md", src);
        assert!(
            keywords(&res).is_empty(),
            "{:#?}",
            res.normative_occurrences
        );
    }

    #[test]
    fn a_keyword_inside_an_inline_code_span_is_exempt() {
        let src = "# Decision\n\nThe token `MUST` is discussed, not asserted, here.\n";
        let res = extract_document("docs/adr/x.md", src);
        assert!(
            keywords(&res).is_empty(),
            "{:#?}",
            res.normative_occurrences
        );
    }

    #[test]
    fn a_keyword_inside_a_fenced_code_block_is_exempt() {
        let src = "# Decision\n\n```text\nMUST\n```\n";
        let res = extract_document("docs/adr/x.md", src);
        assert!(
            keywords(&res).is_empty(),
            "{:#?}",
            res.normative_occurrences
        );
    }

    #[test]
    fn a_keyword_inside_a_claim_blocks_own_yaml_is_exempt() {
        // A claim block's YAML is a fenced code block like any other —
        // exempted the same way, not by a special case for `claim` fences.
        let src = "### [x]\n\n```claim\nkind: constraint\nevaluator: test\n# MUST not appear here anyway, but if it did:\n```\n";
        let res = extract_document("docs/specs/x.md", src);
        assert!(
            keywords(&res).is_empty(),
            "{:#?}",
            res.normative_occurrences
        );
    }

    #[test]
    fn a_keyword_in_a_heading_counts_as_own_voice() {
        // Delegated decision: headings are not in the dispatch's exemption
        // list (block quotes, inline code, fenced code blocks) — only
        // those three are exempt, so a heading is own voice like any
        // other text. A heading asserting "you MUST configure X" is a
        // real normative assertion in prose form, not structurally
        // different from the same sentence in a paragraph.
        let src = "# You MUST configure this first\n";
        let res = extract_document("docs/adr/x.md", src);
        assert_eq!(keywords(&res), vec!["MUST"]);
    }

    #[test]
    fn a_quoted_keyword_and_a_coded_keyword_coexist_with_a_real_own_voice_one() {
        let src = "# Decision\n\n> The old rule said it MUST retry.\n\nWe reject that; note `MUST` above is quoted, not asserted. The new rule\nSHALL retry once.\n";
        let res = extract_document("docs/adr/x.md", src);
        assert_eq!(keywords(&res), vec!["SHALL"]);
    }

    // --- bold-form definitions -------------------------------------------
    //
    // The corpus's own convention (team-lead dispatch): 400/418 real-corpus
    // definitions use this shape, only 18 use heading-form. Four properties
    // together identify one — line start, a `**…**` wrapper, a bracketed
    // kebab id inside it, and definitional punctuation immediately after —
    // and none alone is sufficient, which is what the false-positive tests
    // below are for.

    #[test]
    fn extracts_a_bold_form_claim_with_a_direct_colon() {
        let src = "**[sigil-required]**: An input string MUST be treated as\ncontaining an alias.\n\n```claim\nkind: constraint\nevaluator: test\n```\n";
        let res = extract_document("docs/specs/x.md", src);
        assert_eq!(ids(&res), vec!["sigil-required"]);
        assert!(res.orphan_claims.is_empty(), "{:#?}", res.orphan_claims);
    }

    #[test]
    fn extracts_a_bold_form_claim_with_a_parenthetical_before_the_colon() {
        // The 17-of-400 variant: a parenthetical (which may hold
        // non-ASCII, e.g. a prime) sits between the closing `**` and the
        // colon.
        let src = "**[eos-scheduler-frozen-stability]** (P8): Once an EP\ntransitions, its scope MUST NOT change.\n\n```claim\nkind: constraint\nevaluator: test\n```\n";
        let res = extract_document("docs/specs/x.md", src);
        assert_eq!(ids(&res), vec!["eos-scheduler-frozen-stability"]);

        let src_prime = "**[eos-scheduler-bounded-window]** (P9′): When a ready EP\nhas a feasible worker, it MUST be dispatched.\n\n```claim\nkind: constraint\nevaluator: test\n```\n";
        let res_prime = extract_document("docs/specs/x.md", src_prime);
        assert_eq!(ids(&res_prime), vec!["eos-scheduler-bounded-window"]);
    }

    #[test]
    fn bold_form_ordinary_bold_text_without_brackets_is_not_a_definition() {
        let src = "**Note**: this is emphasis, not an id definition.\n\n```claim\nkind: constraint\nevaluator: test\n```\n";
        let res = extract_document("docs/specs/x.md", src);
        assert!(res.claims.is_empty());
        assert_eq!(res.orphan_claims.len(), 1);
    }

    #[test]
    fn bold_form_mid_sentence_citation_of_a_real_id_is_not_a_second_definition() {
        // Criterion 4's false-positive floor: citing an existing id in
        // running prose — not at line start — must never be mistaken for
        // a definition, even though the bracket-kebab wrapper is
        // identical to a real one.
        let src = "**[real-claim]**: The real definition.\n\n```claim\nkind: constraint\nevaluator: test\n```\n\nAs required by **[real-claim]** above, the caller MUST retry.\n";
        let res = extract_document("docs/specs/x.md", src);
        assert_eq!(ids(&res), vec!["real-claim"]);
    }

    #[test]
    fn bold_form_line_start_bracket_with_no_adjacent_punctuation_is_not_a_definition() {
        // The exact real-corpus shape that motivated the punctuation
        // requirement rather than "bracket-kebab bold at line start"
        // alone: `**[id]**` opens the line but is followed by ordinary
        // prose, not a colon (whether direct or parenthetical).
        let src = "**[not-yet-a-definition]** appears here but is not\nimmediately followed by punctuation.\n\n```claim\nkind: constraint\nevaluator: test\n```\n";
        let res = extract_document("docs/specs/x.md", src);
        assert!(res.claims.is_empty());
        assert_eq!(res.orphan_claims.len(), 1);
    }

    #[test]
    fn bold_form_inside_a_list_item_is_not_line_start() {
        // The real-corpus counterexample (`no-unpublished-dependency`,
        // docs/specs/atom-sourcing.md): a list marker precedes the bold
        // span on the same line, so the byte immediately before `**` is
        // a space, never `\n` — excluded by the line-start check itself,
        // independent of the punctuation floor.
        let src = "- **[no-unpublished-dependency]** MUST be enforced: atoms\n  with an unpublished dependency are rejected.\n\n```claim\nkind: constraint\nevaluator: test\n```\n";
        let res = extract_document("docs/specs/x.md", src);
        assert!(res.claims.is_empty());
        assert_eq!(res.orphan_claims.len(), 1);
    }

    #[test]
    fn bold_form_prose_scope_stops_at_the_next_bold_form_definition() {
        let src = "**[a]**: first claim.\n\n[link-a](target-a)\n\n**[b]**: second claim.\n\n[link-b](target-b)\n\n```claim\nkind: constraint\nevaluator: test\n```\n";
        let res = extract_document("docs/specs/x.md", src);
        let claim_b = res.claims.iter().find(|c| c.id == "b").unwrap();
        assert_eq!(claim_b.prose_links, vec!["target-b"]);
    }

    #[test]
    fn bold_form_prose_scope_stops_at_the_next_heading_of_any_level() {
        let src = "**[a]**: first claim.\n\n[link-a](target-a)\n\n#### Notes\n\n[link-b](target-b)\n\n```claim\nkind: constraint\nevaluator: test\n```\n";
        let res = extract_document("docs/specs/x.md", src);
        // The block's nearest preceding anchor is still [a] (`#### Notes`
        // is not bracket-kebab), but [a]'s own PROSE SCOPE closed at the
        // heading, so only target-a is in scope.
        assert_eq!(ids(&res), vec!["a"]);
        assert_eq!(res.claims[0].prose_links, vec!["target-a"]);
    }

    #[test]
    fn a_claim_block_after_a_bold_form_definition_and_intervening_prose_is_owned_by_it() {
        // Mirrors nested_headings_find_the_nearest_bracket_kebab_ancestor:
        // a bold-form definition's block need not be immediately
        // adjacent, only nearest.
        let src = "**[lock-groundness]**: Every lock value MUST be ground.\n\nSome elaborating prose in between.\n\n```claim\nkind: constraint\nevaluator: test\n```\n";
        let res = extract_document("docs/specs/lock.md", src);
        assert_eq!(ids(&res), vec!["lock-groundness"]);
    }

    #[test]
    fn heading_form_and_bold_form_can_coexist_in_one_document() {
        let src = "### [heading-claim]\n\n```claim\nkind: constraint\nevaluator: test\n```\n\n**[bold-claim]**: A second claim, this time bold-form.\n\n```claim\nkind: constraint\nevaluator: test\n```\n";
        let res = extract_document("docs/specs/x.md", src);
        let mut got = ids(&res);
        got.sort_unstable();
        assert_eq!(got, vec!["bold-claim", "heading-claim"]);
    }

    // --- html-form anchors (`<a id="…"></a>`) ------------------------------

    #[test]
    fn extracts_an_html_anchored_claim() {
        let src = "Some prose.\n\n<a id=\"html-claim\"></a>\nThe system MUST persist keyed data.\n\n[see also](target-a)\n\n```claim\nkind: requirement\nevaluator: test\n```\n";
        let res = extract_document("docs/specs/a.md", src);
        assert_eq!(ids(&res), vec!["html-claim"]);
        assert_eq!(res.claims[0].prose_links, vec!["target-a"]);
    }

    #[test]
    fn an_html_anchor_can_sit_mid_paragraph() {
        // The exact shape the dispatch names: "mid-paragraph, in a table
        // cell, immediately before a claim fence" — unlike heading- and
        // bold-form, which both require line start, an html anchor has no
        // positional constraint at all.
        let src = "Some lead-in text <a id=\"mid-para\"></a> and more prose after it.\n\n```claim\nkind: constraint\nevaluator: test\n```\n";
        let res = extract_document("docs/specs/a.md", src);
        assert_eq!(ids(&res), vec!["mid-para"]);
    }

    #[test]
    fn an_html_anchor_immediately_before_the_claim_fence_still_resolves() {
        let src = "Some prose.\n\n<a id=\"right-before\"></a>\n```claim\nkind: constraint\nevaluator: test\n```\n";
        let res = extract_document("docs/specs/a.md", src);
        assert_eq!(ids(&res), vec!["right-before"]);
    }

    #[test]
    fn an_html_anchors_prose_scope_stops_at_the_next_heading_of_any_level() {
        let src = "<a id=\"a\"></a>\n\n[link-a](target-a)\n\n#### Notes\n\n[link-b](target-b)\n\n```claim\nkind: constraint\nevaluator: test\n```\n";
        let res = extract_document("docs/specs/x.md", src);
        assert_eq!(ids(&res), vec!["a"]);
        assert_eq!(res.claims[0].prose_links, vec!["target-a"]);
    }

    #[test]
    fn an_html_anchors_prose_scope_stops_at_the_next_bold_form_definition() {
        // Cross-form: an html anchor's inline scope closes at ANY sibling
        // inline definition, not only another html anchor. The claim
        // block sits between the two anchors so its owner ([a]) is
        // unambiguous; target-b, positioned after [b] begins, is outside
        // [a]'s scope only if the cross-form stop rule actually fires —
        // absent it, scope_end would default to end-of-document and
        // include target-b too.
        let src = "<a id=\"a\"></a>\n\n[link-a](target-a)\n\n```claim\nkind: constraint\nevaluator: test\n```\n\n**[b]**: second claim.\n\n[link-b](target-b)\n";
        let res = extract_document("docs/specs/x.md", src);
        assert_eq!(ids(&res), vec!["a"]);
        assert_eq!(res.claims[0].prose_links, vec!["target-a"]);
    }

    #[test]
    fn a_bold_form_definitions_prose_scope_now_also_stops_at_the_next_html_anchor() {
        // The symmetric case: adding html-form must not silently widen
        // bold-form's own existing scope rule. Same shape as the mirror
        // test above, forms swapped.
        let src = "**[a]**: first claim.\n\n[link-a](target-a)\n\n```claim\nkind: constraint\nevaluator: test\n```\n\n<a id=\"b\"></a>\n\n[link-b](target-b)\n";
        let res = extract_document("docs/specs/x.md", src);
        assert_eq!(ids(&res), vec!["a"]);
        assert_eq!(res.claims[0].prose_links, vec!["target-a"]);
    }

    #[test]
    fn an_html_anchor_with_no_claim_block_is_unregistered() {
        let src = "<a id=\"orphaned\"></a>\n\nNo block follows.\n";
        let res = extract_document("docs/specs/x.md", src);
        assert!(res.claims.is_empty());
        assert_eq!(res.unregistered_definitions.len(), 1);
        assert_eq!(res.unregistered_definitions[0].id, "orphaned");
    }

    #[test]
    fn a_registered_html_anchor_is_not_reported_as_unregistered() {
        let src = "<a id=\"registered\"></a>\n\nhas a block.\n\n```claim\nkind: constraint\nevaluator: test\n```\n";
        let res = extract_document("docs/specs/x.md", src);
        assert!(res.unregistered_definitions.is_empty(), "{:#?}", res);
    }

    #[test]
    fn an_anchor_tag_with_no_id_attribute_is_never_a_candidate() {
        // The adversarial floor: an ordinary `<a href="…">` link must
        // never register as a definition, not even a malformed one.
        let src = "See <a href=\"https://example.com\">this</a> for background.\n";
        let res = extract_document("docs/specs/x.md", src);
        assert!(res.unregistered_definitions.is_empty());
        assert!(res.malformed_ids.is_empty());
    }

    #[test]
    fn an_empty_id_attribute_is_malformed_not_silently_skipped() {
        let src = "<a id=\"\"></a>\n\nSome prose.\n";
        let res = extract_document("docs/specs/x.md", src);
        assert_eq!(res.malformed_ids.len(), 1, "{:#?}", res.malformed_ids);
        assert_eq!(res.malformed_ids[0].id, "");
        assert!(res.unregistered_definitions.is_empty());
    }

    #[test]
    fn a_non_kebab_id_attribute_is_malformed() {
        let src = "<a id=\"Not_Valid\"></a>\n\nSome prose.\n";
        let res = extract_document("docs/specs/x.md", src);
        assert_eq!(res.malformed_ids.len(), 1);
        assert_eq!(res.malformed_ids[0].id, "Not_Valid");
    }

    #[test]
    fn an_unclosed_anchor_tag_is_silently_not_a_candidate() {
        // No immediately-adjacent `</a>` — never a definition attempt at
        // all (the confirming structural signal never arrived), the same
        // silence an unpunctuated bold span gets. Not malformed, not
        // unregistered: nothing.
        let src = "<a id=\"never-closed\"> and then some prose that never closes it.\n";
        let res = extract_document("docs/specs/x.md", src);
        assert!(res.unregistered_definitions.is_empty());
        assert!(res.malformed_ids.is_empty());
    }

    #[test]
    fn an_anchor_with_real_content_between_the_tags_is_not_a_candidate() {
        // Not the invisible-empty-marker shape this form exists for.
        let src = "<a id=\"has-content\">some text</a>\n\nSome prose.\n";
        let res = extract_document("docs/specs/x.md", src);
        assert!(res.unregistered_definitions.is_empty());
        assert!(res.malformed_ids.is_empty());
    }

    #[test]
    fn heading_bold_and_html_forms_can_all_coexist_in_one_document() {
        let src = "### [heading-claim]\n\n```claim\nkind: constraint\nevaluator: test\n```\n\n**[bold-claim]**: A second claim.\n\n```claim\nkind: constraint\nevaluator: test\n```\n\n<a id=\"html-claim\"></a>\n\nA third claim.\n\n```claim\nkind: constraint\nevaluator: test\n```\n";
        let res = extract_document("docs/specs/x.md", src);
        let mut got = ids(&res);
        got.sort_unstable();
        assert_eq!(got, vec!["bold-claim", "heading-claim", "html-claim"]);
    }

    // --- html anchors adjacent to a heading (the migration's real shape) --
    //
    // The defect the architect's ruling caught: a heading-adjacent anchor
    // treated as inline closes its own scope at the very next heading —
    // its OWN heading, when the anchor precedes it — collapsing
    // `[scope_start, scope_end)` to nothing and silently dropping every
    // link and code span in the section. `heading_adjacent_to`
    // reclassifies these to `AnchorKind::Heading`, inheriting the
    // heading's level and extent, before scope computation ever runs.

    #[test]
    fn an_anchor_immediately_before_its_heading_inherits_the_headings_scope() {
        // The exact shape the migration will produce 473 times: an
        // invisible anchor naming a titled heading's id.
        let src = "<a id=\"lock-sufficiency\"></a>\n#### Lock sufficiency\n\nThe lock MUST pin. [link](x.md)\n\n```claim\nkind: requirement\nevaluator: test\n```\n";
        let res = extract_document("docs/specs/a.md", src);
        assert_eq!(ids(&res), vec!["lock-sufficiency"]);
        assert_eq!(res.claims[0].prose_links, vec!["x.md".to_string()]);
    }

    #[test]
    fn an_anchor_immediately_after_its_heading_also_inherits_the_headings_scope() {
        // The other authoring order — heading first, anchor right after
        // it — must resolve identically.
        let src = "#### Lock sufficiency\n<a id=\"lock-sufficiency\"></a>\n\nThe lock MUST pin. [link](x.md)\n\n```claim\nkind: requirement\nevaluator: test\n```\n";
        let res = extract_document("docs/specs/a.md", src);
        assert_eq!(ids(&res), vec!["lock-sufficiency"]);
        assert_eq!(res.claims[0].prose_links, vec!["x.md".to_string()]);
    }

    #[test]
    fn a_heading_adjacent_anchors_scope_survives_a_deeper_subheading() {
        // Inherited from Heading-form directly, the same property
        // `nested_headings_find_the_nearest_bracket_kebab_ancestor` pins
        // for an ordinary bracket-kebab heading: unlike an inline
        // definition, a section's scope is not cut short by a deeper
        // subheading beneath it.
        let src = "<a id=\"parent\"></a>\n## Parent\n\n[outer](outer-target)\n\n### Child\n\n[inner](inner-target)\n\n```claim\nkind: requirement\nevaluator: test\n```\n";
        let res = extract_document("docs/specs/a.md", src);
        assert_eq!(ids(&res), vec!["parent"]);
        let mut links = res.claims[0].prose_links.clone();
        links.sort_unstable();
        assert_eq!(links, vec!["inner-target", "outer-target"]);
    }

    #[test]
    fn a_heading_adjacent_anchor_does_not_widen_past_the_next_same_level_heading() {
        let src = "<a id=\"a\"></a>\n## A\n\n[link-a](target-a)\n\n```claim\nkind: requirement\nevaluator: test\n```\n\n## B\n\n[link-b](target-b)\n";
        let res = extract_document("docs/specs/a.md", src);
        assert_eq!(ids(&res), vec!["a"]);
        assert_eq!(res.claims[0].prose_links, vec!["target-a".to_string()]);
    }

    #[test]
    fn a_free_standing_html_anchor_still_uses_the_inline_scope_rule() {
        // Not adjacent to any heading — must NOT be reclassified; the
        // existing inline behavior (stops at the next heading of any
        // level or next sibling definition) still applies.
        let src = "Some lead-in prose.\n\n<a id=\"free-standing\"></a>\n\n[link-a](target-a)\n\n#### Notes\n\n[link-b](target-b)\n\n```claim\nkind: constraint\nevaluator: test\n```\n";
        let res = extract_document("docs/specs/a.md", src);
        assert_eq!(ids(&res), vec!["free-standing"]);
        assert_eq!(res.claims[0].prose_links, vec!["target-a".to_string()]);
    }

    // --- heading_adjacent_to's nearest-match fix ---------------------------
    //
    // `position()` picked the FIRST heading in document order satisfying
    // whitespace-adjacency on either side, not the nearest — a real defect
    // whenever an empty (no-body-prose) heading sits immediately beside
    // the anchor on one side and a real heading sits on the other. Every
    // fixture and unit test above has exactly one candidate per anchor, so
    // none of them could have caught this.

    #[test]
    fn an_anchor_tied_between_two_empty_headings_binds_to_the_one_it_precedes() {
        // The scope-collapse shape, exactly: `## A` carries no body prose,
        // then the anchor, then `## B` with real content — an equal
        // whitespace gap on both sides (`\n\n`, the tie the doc comment
        // names). `position()` bound this to A, whose own scope (as a
        // level-2 heading) closes at the very next level-2 heading — B,
        // immediately following — collapsing to nothing and dropping
        // "target" from the claim entirely. The fix must bind to B, the
        // heading the anchor PRECEDES, recovering the link.
        let src = "## A\n\n<a id=\"b\"></a>\n\n## B\n\n[link](target)\n\n```claim\nkind: requirement\nevaluator: test\n```\n";
        let res = extract_document("docs/specs/a.md", src);
        assert_eq!(ids(&res), vec!["b"]);
        assert_eq!(res.claims[0].prose_links, vec!["target".to_string()]);
    }

    #[test]
    fn an_anchor_binds_to_the_heading_with_the_smaller_whitespace_gap() {
        // Not a tie: a wide gap on one side, a tight one on the other —
        // proves the fix compares actual distance, not merely "prefers
        // the following heading" as a blanket rule regardless of gap
        // size.
        let src = "## Far\n\n\n\n<a id=\"near\"></a>\n## Near\n\n[link](target)\n\n```claim\nkind: requirement\nevaluator: test\n```\n";
        let res = extract_document("docs/specs/a.md", src);
        assert_eq!(ids(&res), vec!["near"]);
        assert_eq!(res.claims[0].prose_links, vec!["target".to_string()]);
    }

    #[test]
    fn an_anchor_tied_between_a_parent_and_its_child_binds_to_the_child_not_the_parent() {
        // The over-widening shape: `## Parent` carries no body prose, then
        // the anchor, then `### Child` with content, then an `### Unrelated`
        // sibling. `position()` bound this to Parent, a level-2 heading
        // whose scope runs until the next level-2-or-shallower heading —
        // there is none here, so it swallowed BOTH Child's and Unrelated's
        // links. Binding to Child (level 3) instead closes the scope at
        // Unrelated (also level 3), correctly excluding it.
        let src = "## Parent\n\n<a id=\"child\"></a>\n\n### Child\n\n[inner](inner-target)\n\n```claim\nkind: requirement\nevaluator: test\n```\n\n### Unrelated\n\n[outer](outer-target)\n";
        let res = extract_document("docs/specs/a.md", src);
        assert_eq!(ids(&res), vec!["child"]);
        assert_eq!(res.claims[0].prose_links, vec!["inner-target".to_string()]);
    }

    // --- unregistered definitions (coverage count) ------------------------

    #[test]
    fn a_bold_form_definition_with_no_claim_block_is_unregistered() {
        let src = "**[unregistered-one]**: A definition nobody registered yet.\n";
        let res = extract_document("docs/specs/x.md", src);
        assert!(res.claims.is_empty());
        assert_eq!(res.unregistered_definitions.len(), 1);
        assert_eq!(res.unregistered_definitions[0].id, "unregistered-one");
        assert_eq!(res.unregistered_definitions[0].file, "docs/specs/x.md");
    }

    #[test]
    fn a_heading_form_definition_with_no_claim_block_is_also_unregistered() {
        // The coverage count is generalized across both forms, not
        // bold-only: an id heading with no following block was always
        // silently invisible before this dispatch; it is real corpus
        // debt either way.
        let src = "### [heading-only]\n\nNo block follows.\n";
        let res = extract_document("docs/specs/x.md", src);
        assert!(res.claims.is_empty());
        assert_eq!(res.unregistered_definitions.len(), 1);
        assert_eq!(res.unregistered_definitions[0].id, "heading-only");
    }

    #[test]
    fn a_registered_bold_form_definition_is_not_reported_as_unregistered() {
        let src =
            "**[registered]**: has a block.\n\n```claim\nkind: constraint\nevaluator: test\n```\n";
        let res = extract_document("docs/specs/x.md", src);
        assert!(res.unregistered_definitions.is_empty(), "{:#?}", res);
    }

    #[test]
    fn scan_interstitial_chunk_matches_the_direct_and_parenthetical_forms() {
        assert!(matches!(
            scan_interstitial_chunk(": text", 128),
            InterstitialStep::ResolvedAt(0)
        ));
        assert!(matches!(
            scan_interstitial_chunk(" (P8): text", 128),
            InterstitialStep::ResolvedAt(5)
        ));
    }

    #[test]
    fn scan_interstitial_chunk_keeps_hunting_when_no_colon_yet_within_budget() {
        assert!(matches!(
            scan_interstitial_chunk(" text, no colon", 128),
            InterstitialStep::Continue
        ));
    }

    #[test]
    fn scan_interstitial_chunk_fails_on_a_crossed_line_or_an_exhausted_budget() {
        assert!(matches!(
            scan_interstitial_chunk(" text\nmore: after a break", 128),
            InterstitialStep::Failed
        ));
        assert!(matches!(
            scan_interstitial_chunk(" text, no colon", 4),
            InterstitialStep::Failed
        ));
    }

    #[test]
    fn extracts_a_bold_form_claim_with_an_italicized_revision_note() {
        // The shape this dispatch exists for: a markdown emphasis span
        // (`_(...)_`) between the closing `**` and the colon, carrying
        // revision history rather than a plain parenthetical label.
        // Multi-line, matching the real corpus (e.g.
        // docs/specs/trust-model.md's `[trust-owner-selector]`).
        let src = "**[lock-groundness]** _(amended 2026-07-14 — retitled\nfrom lock-nonzero)_: Every lock value MUST be ground.\n\n```claim\nkind: constraint\nevaluator: test\n```\n";
        let res = extract_document("docs/specs/x.md", src);
        assert_eq!(ids(&res), vec!["lock-groundness"]);
        assert!(res.orphan_claims.is_empty(), "{:#?}", res.orphan_claims);
    }

    #[test]
    fn extracts_a_bold_form_claim_with_a_retired_or_superseded_note() {
        let src = "**[anchor-is-genesis]** _(retired 2026-07-08 — superseded by\nthe charter amendment)_: The former rule is retired.\n\n```claim\nkind: constraint\nevaluator: test\n```\n";
        let res = extract_document("docs/specs/x.md", src);
        assert_eq!(ids(&res), vec!["anchor-is-genesis"]);
    }

    #[test]
    fn a_nested_bracket_id_inside_the_revision_note_does_not_confuse_the_wrapper_skip() {
        // The real corpus's own counterexample
        // (docs/specs/atom-transactions.md's `[anchor-resolvable]`): the
        // aside itself cites another id in brackets, which must stay
        // opaque content — never mistaken for a second definition, never
        // breaking the wrapper scan.
        let src = "**[anchor-resolvable]** _(supersedes [anchor-discoverable],\n2026-07-08)_: Given a source, any party MUST verify.\n\n```claim\nkind: constraint\nevaluator: test\n```\n";
        let res = extract_document("docs/specs/x.md", src);
        assert_eq!(ids(&res), vec!["anchor-resolvable"]);
    }

    #[test]
    fn the_two_already_supported_forms_still_work_alongside_the_italic_one() {
        let src = "**[direct]**: direct colon.\n\n```claim\nkind: constraint\nevaluator: test\n```\n\n**[paren]** (P8): plain parenthetical.\n\n```claim\nkind: constraint\nevaluator: test\n```\n\n**[italic]** _(amended 2026-07-14)_: italic revision note.\n\n```claim\nkind: constraint\nevaluator: test\n```\n";
        let res = extract_document("docs/specs/x.md", src);
        let mut got = ids(&res);
        got.sort_unstable();
        assert_eq!(got, vec!["direct", "italic", "paren"]);
        assert!(res.orphan_claims.is_empty(), "{:#?}", res.orphan_claims);
    }

    // --- malformed-id -------------------------------------------------
    //
    // `.ledger/2026-08-04-malformed-ids-are-silently-invisible.md`: a
    // bracketed token in definition position that fails the id grammar
    // used to produce no diagnostic at all — the same silence a correctly
    // handled definition produces. `malformed_bracket_id`'s own doc
    // comment states the predicate; these tests pin its boundary,
    // negative cases first since a false positive here would make the
    // diagnostic ignorable.

    fn malformed_ids(res: &ExtractResult) -> Vec<&str> {
        res.malformed_ids.iter().map(|m| m.id.as_str()).collect()
    }

    #[test]
    fn malformed_bracket_id_pins_the_predicate_directly() {
        // The exact real-corpus shapes
        // (`.ledger/2026-08-04-malformed-ids-are-silently-invisible.md`):
        // an otherwise-kebab token with one stray uppercase segment.
        assert_eq!(
            malformed_bracket_id("[boundary-L1-concerns]"),
            Some("boundary-L1-concerns")
        );
        assert_eq!(
            malformed_bracket_id("[daemon-discovery-vN]"),
            Some("daemon-discovery-vN")
        );
        // Other non-kebab characters the dispatch names explicitly:
        // underscore, dot — both still id-shaped (one word, no
        // whitespace), so they fire too.
        assert_eq!(
            malformed_bracket_id("[lock_groundness]"),
            Some("lock_groundness")
        );
        assert_eq!(malformed_bracket_id("[v1.2]"), Some("v1.2"));
        // A valid kebab id is never malformed.
        assert_eq!(malformed_bracket_id("[lock-groundness]"), None);
        // Not bracket-shaped at all.
        assert_eq!(malformed_bracket_id("lock-groundness"), None);
        // An empty bracket carries no id-shaped signal at all — likely
        // link/checkbox syntax, not an attempted id.
        assert_eq!(malformed_bracket_id("[]"), None);
        // Whitespace is the prose signal: a multi-word bracket reads as
        // a sentence, not a typo'd id, so it must not fire.
        assert_eq!(malformed_bracket_id("[Note to reader]"), None);
        assert_eq!(malformed_bracket_id("[trailing space ]"), None);
    }

    #[test]
    fn a_malformed_bold_form_id_is_reported_with_the_real_corpus_shape() {
        // The exact shape from the sibling corpus's `layer-boundaries.md`:
        // a bold-form definition whose id carries an uppercase segment.
        let src = "**[boundary-L1-concerns]**: L1 (atom) owns content\naddressing and lock verification.\n";
        let res = extract_document("docs/specs/x.md", src);
        assert_eq!(malformed_ids(&res), vec!["boundary-L1-concerns"]);
        // Not a recognized definition at all — no claim, no
        // unregistered-definition either, matching the ledger's "not
        // reported as malformed, not reported as unregistered" complaint
        // about the OLD (silent) behavior; that gap is now filled by
        // `malformed_ids` specifically, not by widening the other two.
        assert!(res.claims.is_empty());
        assert!(res.unregistered_definitions.is_empty(), "{:#?}", res);
    }

    #[test]
    fn a_malformed_bold_form_id_with_a_parenthetical_is_still_reported() {
        let src = "**[daemon-discovery-vN]** (P8): In future versions, ion\nMAY support additional discovery mechanisms.\n";
        let res = extract_document("docs/specs/x.md", src);
        assert_eq!(malformed_ids(&res), vec!["daemon-discovery-vN"]);
    }

    #[test]
    fn a_malformed_heading_form_id_is_also_reported() {
        // The predicate generalizes across both forms, the same way
        // `unregistered-definition` does.
        let src = "### [Boundary-L1]\n\nSome prose.\n";
        let res = extract_document("docs/specs/x.md", src);
        assert_eq!(malformed_ids(&res), vec!["Boundary-L1"]);
        assert!(res.claims.is_empty());
    }

    #[test]
    fn a_malformed_id_leaves_its_claim_block_orphaned_not_silently_owned() {
        // The defect's real cost: a claim block after a malformed
        // definition, with no other real anchor preceding it, still
        // becomes `orphan-claim` (unchanged behavior — a malformed token
        // was never a valid anchor) — but now the malformed-id diagnostic
        // fires alongside it, naming the actual cause instead of leaving
        // a bare orphan-claim for someone to puzzle over.
        let src = "**[boundary-L1-concerns]**: L1 owns things.\n\n```claim\nkind: constraint\nevaluator: test\n```\n";
        let res = extract_document("docs/specs/x.md", src);
        assert!(res.claims.is_empty());
        assert_eq!(res.orphan_claims.len(), 1);
        assert_eq!(malformed_ids(&res), vec!["boundary-L1-concerns"]);
    }

    #[test]
    fn an_empty_bracket_in_bold_form_position_is_not_reported() {
        let src = "**[]**: whatever this is.\n";
        let res = extract_document("docs/specs/x.md", src);
        assert!(malformed_ids(&res).is_empty(), "{:#?}", res.malformed_ids);
    }

    #[test]
    fn a_multi_word_bracket_reads_as_prose_not_a_malformed_id() {
        // The false-positive floor's central case: a bracket that is
        // plainly a sentence, not a typo'd id — the exact ambiguity the
        // dispatch flagged as needing a boundary decision.
        let src = "**[Note to reader]**: this is prose, not an id.\n";
        let res = extract_document("docs/specs/x.md", src);
        assert!(malformed_ids(&res).is_empty(), "{:#?}", res.malformed_ids);
        // Also not a real definition — this must not silently become one
        // either, which `bold_form_ordinary_bold_text_without_brackets_is_not_a_definition`
        // already covers for the no-bracket case; this pins the
        // bracket-but-prose case the same way.
        assert!(res.claims.is_empty());
        assert_eq!(res.orphan_claims.len(), 0);
    }

    #[test]
    fn ordinary_bold_text_without_brackets_still_reports_no_malformed_id() {
        let src = "**Note**: this is emphasis, not an id definition.\n\n```claim\nkind: constraint\nevaluator: test\n```\n";
        let res = extract_document("docs/specs/x.md", src);
        assert!(malformed_ids(&res).is_empty(), "{:#?}", res.malformed_ids);
    }

    #[test]
    fn a_malformed_looking_bracket_not_line_start_is_not_reported() {
        // Mirrors `bold_form_mid_sentence_citation_of_a_real_id_is_not_a_second_definition`:
        // the same false-positive floor must hold for a malformed-looking
        // token cited mid-sentence, not just a well-formed one.
        let src = "**[boundary-L1-concerns]**: L1 owns things.\n\nAs discussed in **[boundary-L1-concerns]** above, the caller\nMUST retry.\n";
        let res = extract_document("docs/specs/x.md", src);
        // Exactly one report — the line-start definition — not two.
        assert_eq!(malformed_ids(&res), vec!["boundary-L1-concerns"]);
    }

    #[test]
    fn a_malformed_looking_bracket_inside_a_list_item_is_not_reported() {
        // Mirrors `bold_form_inside_a_list_item_is_not_line_start`: a list
        // marker precedes the bold span on the same line, so it is never
        // line-start, independent of the id's grammar.
        let src = "- **[Boundary-L1]** MUST be enforced: things happen.\n";
        let res = extract_document("docs/specs/x.md", src);
        assert!(malformed_ids(&res).is_empty(), "{:#?}", res.malformed_ids);
    }

    #[test]
    fn a_malformed_looking_bracket_with_no_adjacent_punctuation_is_not_reported() {
        // Mirrors `bold_form_line_start_bracket_with_no_adjacent_punctuation_is_not_a_definition`:
        // the definitional-punctuation floor applies identically here —
        // without it, ordinary prose that happens to open a line with a
        // bracket would false-positive.
        let src =
            "**[Boundary-L1]** appears here but is not\nimmediately followed by punctuation.\n";
        let res = extract_document("docs/specs/x.md", src);
        assert!(malformed_ids(&res).is_empty(), "{:#?}", res.malformed_ids);
    }

    #[test]
    fn a_heading_with_prose_around_a_malformed_bracket_is_not_reported() {
        // Heading-form's own structural floor: the check applies only
        // when the ENTIRE heading text is the bracketed token, exactly
        // like `bracket_kebab_id` already requires for a real
        // definition — a heading that merely mentions a bracket in
        // passing is not a definition attempt.
        let src = "## About [Boundary-L1] and other things\n";
        let res = extract_document("docs/specs/x.md", src);
        assert!(malformed_ids(&res).is_empty(), "{:#?}", res.malformed_ids);
    }

    #[test]
    fn a_valid_kebab_id_is_never_also_reported_as_malformed() {
        // Regression floor: every existing recognized-definition path
        // (registered or unregistered, either form) must produce zero
        // malformed-id diagnostics.
        let src = "### [heading-claim]\n\n```claim\nkind: constraint\nevaluator: test\n```\n\n**[bold-claim]**: A second claim, this time bold-form.\n\n```claim\nkind: constraint\nevaluator: test\n```\n\n**[unregistered-bold]**: no block follows.\n";
        let res = extract_document("docs/specs/x.md", src);
        assert!(res.malformed_ids.is_empty(), "{:#?}", res.malformed_ids);
    }

    // --- find_rename_sites --------------------------------------------

    fn substituted(source: &str, target_id: &str, new_id: &str) -> String {
        let mut sites = find_rename_sites(source, target_id);
        sites.sort_by_key(|s| std::cmp::Reverse(s.start));
        let mut out = source.to_string();
        for site in sites {
            assert_eq!(&out[site.start..site.end], target_id);
            out.replace_range(site.start..site.end, new_id);
        }
        out
    }

    #[test]
    fn finds_the_heading_form_definition_site() {
        let src = "### [old-id]\n\nSome prose.\n";
        let sites = find_rename_sites(src, "old-id");
        assert_eq!(sites.len(), 1);
        assert_eq!(sites[0].kind, RenameSiteKind::Definition);
        assert_eq!(&src[sites[0].start..sites[0].end], "old-id");
        assert_eq!(
            substituted(src, "old-id", "new-id"),
            "### [new-id]\n\nSome prose.\n"
        );
    }

    #[test]
    fn finds_the_bold_form_definition_site() {
        let src = "**[old-id]**: Every lock value MUST be ground.\n";
        let sites = find_rename_sites(src, "old-id");
        assert_eq!(sites.len(), 1);
        assert_eq!(sites[0].kind, RenameSiteKind::Definition);
        assert_eq!(
            substituted(src, "old-id", "new-id"),
            "**[new-id]**: Every lock value MUST be ground.\n"
        );
    }

    #[test]
    fn finds_the_html_form_definition_site_without_touching_the_wrapper() {
        let src = "<a id=\"old-id\"></a>\n\nProse.\n";
        let sites = find_rename_sites(src, "old-id");
        assert_eq!(sites.len(), 1);
        assert_eq!(sites[0].kind, RenameSiteKind::Definition);
        assert_eq!(
            substituted(src, "old-id", "new-id"),
            "<a id=\"new-id\"></a>\n\nProse.\n"
        );
    }

    #[test]
    fn html_form_ignores_a_coincidental_data_id_with_the_same_value() {
        // The false-positive floor `find_html_id_attr_span` exists for:
        // a `data-id` attribute happening to carry the exact same value
        // must never be mistaken for the real `id` attribute.
        let src = "<a data-id=\"old-id\" id=\"old-id\"></a>\n";
        let sites = find_rename_sites(src, "old-id");
        assert_eq!(sites.len(), 1);
        // The located span must be the REAL `id=` attribute's value, not
        // `data-id`'s — confirmed by checking the byte immediately before
        // the located span's `id="` is whitespace, not `-`.
        let before = src.as_bytes()[sites[0].start - 5]; // one byte before `id="`
        assert!(
            before.is_ascii_whitespace(),
            "matched data-id instead: {before}"
        );
    }

    #[test]
    fn finds_a_depends_entry_naming_the_claim_id_bare() {
        let src = "### [a]\n\n```claim\nkind: constraint\nevaluator: test\ndepends: [old-id, other-thing]\n```\n";
        let sites = find_rename_sites(src, "old-id");
        assert_eq!(sites.len(), 1);
        assert_eq!(sites[0].kind, RenameSiteKind::Depends);
        assert_eq!(&src[sites[0].start..sites[0].end], "old-id");
    }

    #[test]
    fn finds_a_because_entry_separately_from_depends() {
        let src = "### [a]\n\n```claim\nkind: constraint\nevaluator: test\ndepends: [other-thing]\nbecause: [old-id]\n```\n";
        let sites = find_rename_sites(src, "old-id");
        assert_eq!(sites.len(), 1);
        assert_eq!(sites[0].kind, RenameSiteKind::Because);
    }

    #[test]
    fn a_doc_anchor_entry_whose_anchor_text_coincides_is_not_a_depends_site() {
        // `path#old-id` names a SECTION, not this claim — only a bare
        // entry (no `#`) is a citation of the claim id itself.
        let src = "### [a]\n\n```claim\nkind: constraint\nevaluator: test\ndepends: [docs/x#old-id]\n```\n";
        assert!(find_rename_sites(src, "old-id").is_empty());
    }

    #[test]
    fn finds_a_bare_id_prose_link() {
        let src = "### [a]\n\nSee [the claim](old-id) for detail.\n\n```claim\nkind: constraint\nevaluator: test\n```\n";
        let sites = find_rename_sites(src, "old-id");
        assert_eq!(sites.len(), 1);
        assert_eq!(sites[0].kind, RenameSiteKind::ProseLink);
        assert_eq!(
            substituted(src, "old-id", "new-id"),
            "### [a]\n\nSee [the claim](new-id) for detail.\n\n```claim\nkind: constraint\nevaluator: test\n```\n"
        );
    }

    #[test]
    fn finds_an_anchor_form_prose_link_naming_the_id_as_the_fragment() {
        let src = "See [the claim](docs/specs/x.md#old-id) for detail.\n";
        let sites = find_rename_sites(src, "old-id");
        assert_eq!(sites.len(), 1);
        assert_eq!(sites[0].kind, RenameSiteKind::ProseLink);
        assert_eq!(
            substituted(src, "old-id", "new-id"),
            "See [the claim](docs/specs/x.md#new-id) for detail.\n"
        );
    }

    #[test]
    fn a_bare_fragment_link_also_resolves() {
        let src = "See [the claim](#old-id) for detail.\n";
        let sites = find_rename_sites(src, "old-id");
        assert_eq!(sites.len(), 1);
        assert_eq!(
            substituted(src, "old-id", "new-id"),
            "See [the claim](#new-id) for detail.\n"
        );
    }

    #[test]
    fn a_link_to_an_unrelated_target_is_not_a_site() {
        let src = "See [the claim](other-id) and [more](docs/x#other-id).\n";
        assert!(find_rename_sites(src, "old-id").is_empty());
    }

    #[test]
    fn an_id_substring_inside_ordinary_prose_is_never_a_site() {
        // "old-id" appearing as plain prose text, or as a longer word's
        // substring, must never be mistaken for a definition or a
        // reference — only grammar-recognized positions count.
        let src = "### [a]\n\nThe id old-id-extended is mentioned here, and so is old-id in prose, unlinked.\n\n```claim\nkind: constraint\nevaluator: test\n```\n";
        assert!(find_rename_sites(src, "old-id").is_empty());
    }

    #[test]
    fn all_three_forms_and_every_reference_kind_together() {
        // The dispatch's own required shape: one corpus exercising every
        // site kind in a single document.
        let src = "\
### [old-id]

Some prose about it. See [the claim](old-id) and
[the anchor form](docs/x.md#old-id).

```claim
kind: constraint
evaluator: test
```

### [b]

```claim
kind: constraint
evaluator: test
depends: [old-id]
because: [old-id]
```
";
        let sites = find_rename_sites(src, "old-id");
        let kinds: Vec<_> = sites.iter().map(|s| s.kind).collect();
        assert_eq!(
            kinds,
            vec![
                RenameSiteKind::Definition,
                RenameSiteKind::ProseLink,
                RenameSiteKind::ProseLink,
                RenameSiteKind::Depends,
                RenameSiteKind::Because,
            ]
        );
        let rewritten = substituted(src, "old-id", "new-id");
        assert!(!rewritten.contains("old-id"), "{rewritten}");
        assert_eq!(rewritten.matches("new-id").count(), 5);
    }
}
