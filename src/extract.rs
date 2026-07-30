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

use crate::model::{CiteRef, Claim, Heading, Line, RawClaimBlock};
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

#[derive(Debug, Clone, Default)]
pub struct ExtractResult {
    pub headings: Vec<Heading>,
    pub claims: Vec<Claim>,
    pub orphan_claims: Vec<OrphanClaim>,
    pub normative_occurrences: Vec<NormativeOccurrence>,
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

/// Whether `text` is exactly a bracketed kebab-case token, e.g. `[my-id]`.
/// Returns the inner id, without brackets.
fn bracket_kebab_id(text: &str) -> Option<String> {
    let inner = text.strip_prefix('[')?.strip_suffix(']')?;
    if inner.is_empty() {
        return None;
    }
    let is_kebab = inner.split('-').all(|seg| {
        !seg.is_empty()
            && seg
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
    });
    is_kebab.then(|| inner.to_string())
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

struct RawBlock {
    start: usize,
    end: usize,
    is_claim: bool,
    yaml: String,
}

/// Parse one markdown document and extract its claim blocks, per MVP.md
/// §1.1. `file` is the corpus-relative path recorded on each claim.
pub fn extract_document(file: &str, source: &str) -> ExtractResult {
    let line_index = LineIndex::new(source);
    let parser = Parser::new_ext(source, Options::empty()).into_offset_iter();

    let mut raw_headings: Vec<RawHeading> = Vec::new();
    let mut raw_links: Vec<RawLink> = Vec::new();
    let mut raw_blocks: Vec<RawBlock> = Vec::new();

    // pulldown-cmark's offset iterator gives Start and End the same full
    // element range, so we capture level/start/end at Start and only
    // accumulate text until End closes it.
    let mut cur_heading: Option<(u8, usize, usize, String)> = None;
    let mut cur_block: Option<(usize, usize, bool, String)> = None;
    // Nesting depth, not a bool: a quote can contain a quote. Only its
    // zero/nonzero state matters to the normative-prose scan below.
    let mut blockquote_depth: u32 = 0;
    let mut normative_occurrences: Vec<NormativeOccurrence> = Vec::new();

    for (event, range) in parser {
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
            Event::Start(Tag::Link { dest_url, .. }) => {
                raw_links.push(RawLink {
                    start: range.start,
                    dest: dest_url.into_string(),
                });
            }
            Event::Text(t) => {
                if let Some((_, _, _, ref mut text)) = cur_heading {
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
                // Inline code spans occur only in inline (heading/prose)
                // context; a fenced block's content is Text, never Code.
                if let Some((_, _, _, ref mut text)) = cur_heading {
                    text.push_str(&t);
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

    let headings: Vec<Heading> = raw_headings
        .iter()
        .map(|h| Heading {
            level: h.level,
            text: h.text.clone(),
            line: line_index.line_of(h.start),
        })
        .collect();

    let mut claims = Vec::new();
    let mut orphan_claims = Vec::new();

    for block in raw_blocks.iter().filter(|b| b.is_claim) {
        let block_line = line_index.line_of(block.start);

        // Nearest preceding heading whose text is exactly a bracketed
        // kebab-case token (MVP.md §1.1) — scan every heading before the
        // block, across all levels, keeping the last (= nearest) match.
        let found = raw_headings
            .iter()
            .enumerate()
            .take_while(|(_, h)| h.start < block.start)
            .filter_map(|(i, h)| bracket_kebab_id(&h.text).map(|id| (i, id)))
            .last();

        let Some((idx, id)) = found else {
            orphan_claims.push(OrphanClaim {
                file: file.to_string(),
                line: block_line,
            });
            continue;
        };

        let heading = &raw_headings[idx];
        let heading_line = line_index.line_of(heading.start);

        // Prose scope (C5, §3): from the id heading to the next heading
        // of the same or higher level, excluding the claim block itself.
        let scope_start = heading.end;
        let scope_end = raw_headings[idx + 1..]
            .iter()
            .find(|h2| h2.level <= heading.level)
            .map(|h2| h2.start)
            .unwrap_or(source.len());

        let prose_links: Vec<String> = raw_links
            .iter()
            .filter(|l| l.start >= scope_start && l.start < scope_end)
            .filter(|l| !(l.start >= block.start && l.start < block.end))
            .filter(|l| !is_external(&l.dest))
            .map(|l| l.dest.clone())
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
        });
    }

    ExtractResult {
        headings,
        claims,
        orphan_claims,
        normative_occurrences,
    }
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
}
