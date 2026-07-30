//! The extractor: MVP.md §1.1 (claim block syntax + id assignment) and
//! the C5 prose-link scope (§3).
//!
//! Parses markdown to pulldown-cmark's event tree and walks it — no
//! regular expressions over prose (MVP.md §7). The one regex-shaped bit of
//! parsing here, [`leading_numeral`], operates on an already-isolated
//! heading string, not on document structure, and is implemented by hand
//! rather than pulling in a regex crate for one small token grammar.

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

#[derive(Debug, Clone, Default)]
pub struct ExtractResult {
    pub headings: Vec<Heading>,
    pub claims: Vec<Claim>,
    pub orphan_claims: Vec<OrphanClaim>,
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
    let cites = mapping
        .and_then(|m| m.get("cites"))
        .and_then(|v| v.as_sequence())
        .map(|seq| {
            seq.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    RawClaimBlock {
        yaml: yaml.to_string(),
        kind,
        evaluator,
        cites,
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

    for (event, range) in parser {
        match event {
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
        let cites = raw.cites.iter().map(|c| CiteRef::parse(c)).collect();

        claims.push(Claim {
            id,
            file: file.to_string(),
            heading_line,
            block_line,
            raw,
            cites,
            prose_links,
        });
    }

    ExtractResult {
        headings,
        claims,
        orphan_claims,
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
        let src = "### [lock-groundness]\n\nEvery lock value MUST be ground.\n\n```claim\nkind: constraint\nevaluator: property-test\ncites: [composition-model#6]\n```\n";
        let res = extract_document("docs/specs/lock.md", src);
        assert_eq!(ids(&res), vec!["lock-groundness"]);
        let claim = &res.claims[0];
        assert_eq!(claim.raw.kind.as_deref(), Some("constraint"));
        assert_eq!(claim.raw.evaluator.as_deref(), Some("property-test"));
        assert_eq!(
            claim.cites,
            vec![CiteRef::DocAnchor {
                stem: "composition-model".into(),
                anchor: "6".into()
            }]
        );
        assert!(res.orphan_claims.is_empty());
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
        let src = "### [x]\n\nSee [unrelated](../not-in-corpus.md) and [the web](https://example.com).\n\n```claim\nkind: constraint\ncites: []\n```\n";
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
}
