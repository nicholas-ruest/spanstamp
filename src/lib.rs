//! Parser-derived, content-addressed anchors for Markdown review workflows.

use pulldown_cmark::{Event, Options, Parser, Tag};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::ops::Range;
use thiserror::Error;

/// Current JSON schema version for indexes, anchors, and resolutions.
pub const SCHEMA_VERSION: u32 = 1;

/// A parser-derived index of the reviewable top-level blocks in a document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DocumentIndex {
    pub schema_version: u32,
    pub document_sha256: String,
    pub line_count: usize,
    pub blocks: Vec<Block>,
}

/// One top-level Markdown block and its stable fingerprints.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Block {
    pub index: usize,
    pub kind: String,
    pub start_line: usize,
    pub end_line: usize,
    pub source_sha256: String,
    pub content_sha256: String,
    pub preview: String,
}

/// A portable review target captured from one indexed block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Anchor {
    pub schema_version: u32,
    pub document_sha256: String,
    pub original_block_index: usize,
    pub kind: String,
    pub original_start_line: usize,
    pub original_end_line: usize,
    pub source_sha256: String,
    pub content_sha256: String,
    pub previous_content_sha256: Option<String>,
    pub next_content_sha256: Option<String>,
    pub preview: String,
}

/// Result of resolving an anchor against the current document revision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Resolution {
    pub schema_version: u32,
    pub status: ResolutionStatus,
    pub reason: String,
    pub candidates: Vec<ResolutionCandidate>,
}

/// Whether an anchor has exactly one defensible destination.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResolutionStatus {
    Resolved,
    Ambiguous,
    Missing,
}

/// A block that may satisfy an anchor, with its contextual match score.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolutionCandidate {
    pub block_index: usize,
    pub kind: String,
    pub start_line: usize,
    pub end_line: usize,
    pub source_sha256: String,
    pub context_matches: u8,
    pub preview: String,
}

/// Error returned when a source line does not identify one reviewable block.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CaptureError {
    #[error("line numbers are 1-based; received 0")]
    ZeroLine,
    #[error("line {line} is outside the document (line count: {line_count})")]
    OutsideDocument { line: usize, line_count: usize },
    #[error("line {line} does not contain a reviewable Markdown block")]
    NoBlock { line: usize },
}

#[derive(Debug)]
struct ActiveBlock {
    kind: String,
    start: usize,
    depth: usize,
    content: String,
}

/// Build a deterministic top-level block index from Markdown source.
pub fn index_markdown(source: &str) -> DocumentIndex {
    let parser = Parser::new_ext(source, Options::all()).into_offset_iter();
    let mut blocks = Vec::new();
    let mut active: Option<ActiveBlock> = None;

    for (event, range) in parser {
        if let Some(current) = active.as_mut() {
            match &event {
                Event::Start(tag) => {
                    current.depth += 1;
                    if is_content_boundary(tag) {
                        current.content.push(' ');
                    }
                }
                Event::End(_) => {
                    current.depth -= 1;
                    if current.depth == 0 {
                        let finished = active.take().expect("active block exists");
                        blocks.push(make_block(source, finished, range.end, blocks.len()));
                        continue;
                    }
                }
                _ => append_content(&mut current.content, &event),
            }
            continue;
        }

        match event {
            Event::Start(tag) => {
                if let Some(kind) = top_level_kind(&tag) {
                    active = Some(ActiveBlock {
                        kind,
                        start: range.start,
                        depth: 1,
                        content: String::new(),
                    });
                }
            }
            Event::Rule => blocks.push(make_standalone_block(
                source,
                range,
                "thematic-break",
                "thematic break",
                blocks.len(),
            )),
            _ => {}
        }
    }

    DocumentIndex {
        schema_version: SCHEMA_VERSION,
        document_sha256: sha256(source.as_bytes()),
        line_count: source.lines().count(),
        blocks,
    }
}

/// Capture the block containing a 1-based source line.
pub fn capture_anchor(index: &DocumentIndex, line: usize) -> Result<Anchor, CaptureError> {
    if line == 0 {
        return Err(CaptureError::ZeroLine);
    }
    if line > index.line_count {
        return Err(CaptureError::OutsideDocument {
            line,
            line_count: index.line_count,
        });
    }

    let block = index
        .blocks
        .iter()
        .find(|block| block.start_line <= line && line <= block.end_line)
        .ok_or(CaptureError::NoBlock { line })?;

    Ok(Anchor {
        schema_version: SCHEMA_VERSION,
        document_sha256: index.document_sha256.clone(),
        original_block_index: block.index,
        kind: block.kind.clone(),
        original_start_line: block.start_line,
        original_end_line: block.end_line,
        source_sha256: block.source_sha256.clone(),
        content_sha256: block.content_sha256.clone(),
        previous_content_sha256: block
            .index
            .checked_sub(1)
            .and_then(|position| index.blocks.get(position))
            .map(|neighbor| neighbor.content_sha256.clone()),
        next_content_sha256: index
            .blocks
            .get(block.index + 1)
            .map(|neighbor| neighbor.content_sha256.clone()),
        preview: block.preview.clone(),
    })
}

/// Resolve an anchor without guessing when duplicate content remains tied.
pub fn resolve_anchor(index: &DocumentIndex, anchor: &Anchor) -> Resolution {
    let mut candidates: Vec<ResolutionCandidate> = index
        .blocks
        .iter()
        .filter(|block| block.kind == anchor.kind && block.content_sha256 == anchor.content_sha256)
        .map(|block| ResolutionCandidate {
            block_index: block.index,
            kind: block.kind.clone(),
            start_line: block.start_line,
            end_line: block.end_line,
            source_sha256: block.source_sha256.clone(),
            context_matches: context_score(index, block.index, anchor),
            preview: block.preview.clone(),
        })
        .collect();

    match candidates.len() {
        0 => Resolution {
            schema_version: SCHEMA_VERSION,
            status: ResolutionStatus::Missing,
            reason: "no block has the anchor's kind and semantic content digest".into(),
            candidates,
        },
        1 => Resolution {
            schema_version: SCHEMA_VERSION,
            status: ResolutionStatus::Resolved,
            reason: "one block has the anchor's kind and semantic content digest".into(),
            candidates,
        },
        _ => {
            candidates.sort_by_key(|candidate| {
                (
                    std::cmp::Reverse(candidate.context_matches),
                    candidate.block_index,
                )
            });
            let best = candidates[0].context_matches;
            let best_count = candidates
                .iter()
                .take_while(|candidate| candidate.context_matches == best)
                .count();

            if best > 0 && best_count == 1 {
                candidates.truncate(1);
                Resolution {
                    schema_version: SCHEMA_VERSION,
                    status: ResolutionStatus::Resolved,
                    reason: format!(
                        "duplicate content was disambiguated by {best} neighboring block digest(s)"
                    ),
                    candidates,
                }
            } else {
                Resolution {
                    schema_version: SCHEMA_VERSION,
                    status: ResolutionStatus::Ambiguous,
                    reason: "multiple blocks remain tied after neighboring-context comparison"
                        .into(),
                    candidates,
                }
            }
        }
    }
}

fn make_block(source: &str, active: ActiveBlock, end: usize, index: usize) -> Block {
    let range = active.start..end;
    let semantic = normalize_content(&active.content);
    make_block_from_parts(source, range, active.kind, &semantic, index)
}

fn make_standalone_block(
    source: &str,
    range: Range<usize>,
    kind: &str,
    content: &str,
    index: usize,
) -> Block {
    make_block_from_parts(source, range, kind.to_owned(), content, index)
}

fn make_block_from_parts(
    source: &str,
    mut range: Range<usize>,
    kind: String,
    semantic: &str,
    index: usize,
) -> Block {
    while range.end > range.start && matches!(source.as_bytes()[range.end - 1], b'\n' | b'\r') {
        range.end -= 1;
    }
    let raw = &source[range.clone()];
    let start_line = line_for_offset(source, range.start);
    let end_offset = range.end.saturating_sub(1).max(range.start);
    let end_line = line_for_offset(source, end_offset);
    let content_sha256 = sha256(format!("{kind}\0{semantic}").as_bytes());

    Block {
        index,
        kind,
        start_line,
        end_line,
        source_sha256: sha256(raw.as_bytes()),
        content_sha256,
        preview: preview(semantic),
    }
}

fn top_level_kind(tag: &Tag<'_>) -> Option<String> {
    match tag {
        Tag::Paragraph => Some("paragraph".into()),
        Tag::Heading { level, .. } => Some(format!("heading-{level}")),
        Tag::BlockQuote(_) => Some("block-quote".into()),
        Tag::CodeBlock(_) => Some("code-block".into()),
        Tag::HtmlBlock => Some("html-block".into()),
        Tag::List(Some(_)) => Some("ordered-list".into()),
        Tag::List(None) => Some("unordered-list".into()),
        Tag::FootnoteDefinition(_) => Some("footnote-definition".into()),
        Tag::DefinitionList => Some("definition-list".into()),
        Tag::Table(_) => Some("table".into()),
        Tag::MetadataBlock(_) => Some("metadata-block".into()),
        Tag::Item
        | Tag::DefinitionListTitle
        | Tag::DefinitionListDefinition
        | Tag::TableHead
        | Tag::TableRow
        | Tag::TableCell
        | Tag::Emphasis
        | Tag::Strong
        | Tag::Strikethrough
        | Tag::Link { .. }
        | Tag::Image { .. } => None,
    }
}

fn append_content(target: &mut String, event: &Event<'_>) {
    match event {
        Event::Text(value)
        | Event::Code(value)
        | Event::InlineMath(value)
        | Event::DisplayMath(value)
        | Event::Html(value)
        | Event::InlineHtml(value)
        | Event::FootnoteReference(value) => {
            target.push_str(value);
        }
        Event::SoftBreak | Event::HardBreak => target.push(' '),
        Event::Rule => target.push_str(" thematic break "),
        Event::TaskListMarker(checked) => {
            target.push_str(if *checked { " checked " } else { " unchecked " });
        }
        Event::Start(_) | Event::End(_) => {}
    }
}

fn is_content_boundary(tag: &Tag<'_>) -> bool {
    !matches!(
        tag,
        Tag::Emphasis | Tag::Strong | Tag::Strikethrough | Tag::Link { .. } | Tag::Image { .. }
    )
}

fn normalize_content(content: &str) -> String {
    content.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn preview(content: &str) -> String {
    const LIMIT: usize = 96;
    let mut characters = content.chars();
    let prefix: String = characters.by_ref().take(LIMIT).collect();
    if characters.next().is_some() {
        format!("{prefix}…")
    } else {
        prefix
    }
}

fn line_for_offset(source: &str, offset: usize) -> usize {
    source.as_bytes()[..offset.min(source.len())]
        .iter()
        .filter(|byte| **byte == b'\n')
        .count()
        + 1
}

fn context_score(index: &DocumentIndex, position: usize, anchor: &Anchor) -> u8 {
    let previous = position
        .checked_sub(1)
        .and_then(|candidate| index.blocks.get(candidate))
        .map(|block| block.content_sha256.as_str());
    let next = index
        .blocks
        .get(position + 1)
        .map(|block| block.content_sha256.as_str());

    u8::from(previous == anchor.previous_content_sha256.as_deref())
        + u8::from(next == anchor.next_content_sha256.as_deref())
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indexes_top_level_blocks_and_lines() {
        let source = "# Title\n\nFirst *paragraph*.\n\n- one\n- two\n\n```rust\nlet x = 1;\n```\n";
        let index = index_markdown(source);

        assert_eq!(index.line_count, 10);
        assert_eq!(index.blocks.len(), 4);
        assert_eq!(index.blocks[0].kind, "heading-h1");
        assert_eq!(
            (index.blocks[0].start_line, index.blocks[0].end_line),
            (1, 1)
        );
        assert_eq!(index.blocks[1].preview, "First paragraph.");
        assert_eq!(index.blocks[2].kind, "unordered-list");
        assert_eq!(
            (index.blocks[2].start_line, index.blocks[2].end_line),
            (5, 6)
        );
        assert_eq!(index.blocks[3].kind, "code-block");
    }

    #[test]
    fn resolves_a_block_after_lines_are_inserted() {
        let original = index_markdown("# Notes\n\nKeep this review target.\n");
        let anchor = capture_anchor(&original, 3).unwrap();
        let revised = index_markdown("Intro paragraph.\n\n# Notes\n\nKeep this review target.\n");

        let resolution = resolve_anchor(&revised, &anchor);
        assert_eq!(resolution.status, ResolutionStatus::Resolved);
        assert_eq!(resolution.candidates[0].start_line, 5);
    }

    #[test]
    fn semantic_digest_survives_inline_formatting_changes() {
        let original = index_markdown("A **durable** anchor.\n");
        let anchor = capture_anchor(&original, 1).unwrap();
        let revised = index_markdown("A _durable_ anchor.\n");

        let resolution = resolve_anchor(&revised, &anchor);
        assert_eq!(resolution.status, ResolutionStatus::Resolved);
        assert_ne!(anchor.source_sha256, resolution.candidates[0].source_sha256);
    }

    #[test]
    fn neighboring_context_disambiguates_duplicate_content() {
        let original = index_markdown("Alpha.\n\nRepeat me.\n\nOmega.\n");
        let anchor = capture_anchor(&original, 3).unwrap();
        let revised = index_markdown("Other.\n\nRepeat me.\n\nAlpha.\n\nRepeat me.\n\nOmega.\n");

        let resolution = resolve_anchor(&revised, &anchor);
        assert_eq!(resolution.status, ResolutionStatus::Resolved);
        assert_eq!(resolution.candidates[0].start_line, 7);
        assert_eq!(resolution.candidates[0].context_matches, 2);
    }

    #[test]
    fn tied_duplicate_context_is_ambiguous() {
        let original = index_markdown("Alpha.\n\nRepeat me.\n\nAlpha.\n\nRepeat me.\n\nAlpha.\n");
        let anchor = capture_anchor(&original, 3).unwrap();

        let resolution = resolve_anchor(&original, &anchor);
        assert_eq!(resolution.status, ResolutionStatus::Ambiguous);
        assert_eq!(resolution.candidates.len(), 2);
    }

    #[test]
    fn changed_content_is_missing() {
        let original = index_markdown("Review this exact claim.\n");
        let anchor = capture_anchor(&original, 1).unwrap();
        let revised = index_markdown("Review a different claim.\n");

        assert_eq!(
            resolve_anchor(&revised, &anchor).status,
            ResolutionStatus::Missing
        );
    }

    #[test]
    fn rejects_blank_and_out_of_range_lines() {
        let index = index_markdown("Paragraph.\n\nSecond.\n");
        assert_eq!(capture_anchor(&index, 0), Err(CaptureError::ZeroLine));
        assert_eq!(
            capture_anchor(&index, 2),
            Err(CaptureError::NoBlock { line: 2 })
        );
        assert_eq!(
            capture_anchor(&index, 4),
            Err(CaptureError::OutsideDocument {
                line: 4,
                line_count: 3
            })
        );
    }
}
