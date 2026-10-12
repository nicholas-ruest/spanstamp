# spanstamp

`spanstamp` creates durable, content-addressed review anchors for Markdown. It
uses the Markdown parser's top-level block boundaries—not a line-regex—to bind
a comment to semantic content, its source bytes, block kind, and neighboring
block digests. After a document changes, it relocates the anchor only when the
match is unique.

This is useful for code-review bots, generated-report approval flows, and any
workflow where a saved line number can silently drift after edits.

## What works

- indexes headings, paragraphs, lists, block quotes, code blocks, tables,
  footnotes, definition lists, HTML blocks, metadata blocks, and thematic breaks;
- records 1-based inclusive source lines, source SHA-256, semantic SHA-256, and
  a short preview for each top-level block;
- captures a portable JSON anchor from any line inside a reviewable block;
- resolves moved blocks after surrounding insertions and formatting-only inline
  changes;
- uses previous/next block digests to disambiguate repeated content;
- returns exit `0` only for a unique resolution, exit `2` for `missing` or
  `ambiguous`, and exit `1` for invalid input or I/O errors.

`spanstamp` never fuzzy-matches edited claims. If content changes or duplicate
context stays tied, the caller must recapture or decide explicitly.

## Install

Requires stable Rust 1.85 or newer.

```sh
cargo install --git https://github.com/nicholas-ruest/spanstamp
```

For local development:

```sh
git clone https://github.com/nicholas-ruest/spanstamp.git
cd spanstamp
cargo build --release
```

## Usage

Inspect the parser-derived block map:

```sh
spanstamp index examples/original.md
```

Capture the paragraph on line 3:

```sh
spanstamp capture examples/original.md --line 3 --output review.anchor.json
```

Resolve it after lines have moved:

```sh
spanstamp resolve examples/revised.md review.anchor.json
# RESOLVED: one block has the anchor's kind and semantic content digest
#   lines 5-5 paragraph context=2 digest=... preview="The canary ..."
```

For automation, request JSON and rely on the exit status:

```sh
spanstamp resolve report.md review.anchor.json --format json
```

The JSON formats carry `schema_version: 1` and reject unknown anchor fields.

## Verification

The CI workflow repeats the required stable-Rust gates:

```sh
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
```

## Limits

- Anchors target top-level parser blocks, not arbitrary inline token spans.
- Semantic normalization collapses whitespace and inline presentation markup;
  exact source bytes remain separately fingerprinted.
- Content edits intentionally produce `missing`; there is no similarity search.
- Duplicate blocks resolve only when neighboring block digests yield one best
  candidate. Ties remain `ambiguous`.
- The tool reads local UTF-8 Markdown and does not fetch remote documents,
  store review comments, or apply edits.

## License

[MIT](LICENSE)
