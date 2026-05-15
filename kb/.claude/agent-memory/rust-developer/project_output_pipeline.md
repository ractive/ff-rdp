---
name: iter-60 output format changes
description: Compact envelope, --verbose for meta.connection, --format html escape hatch, jq+text combo
type: project
---

# iter-60 output format architecture

**Why:** Pre-1.0 breaking change to reduce LLM agent token cost. Every response carried ~400 bytes of meta.connection boilerplate and dom returned raw HTML.

## Key changes landed

- `meta.connection` omitted by default; `--verbose` restores it
- `meta` omitted from envelope when empty (`{}`)
- `--format html` added as escape hatch for `dom`/`snapshot` (raw HTML output)
- `--jq` + `--format text` now allowed (jq runs first, then text rendering)
- `dom` default output: ARIA-tree JSON `{ref, role, name, level, state, tag, attrs}`
- `OutputFormat::Html` added to `output_pipeline::OutputFormat` enum

## Architecture

- `connection_meta::merge_into_if_verbose()` — the new call site for all commands (checks `cli.is_verbose()`)
- `connection_meta::is_meta_empty()` — used by `output::envelope` to omit empty meta
- `dom::ARIA_TREE_JS_TEMPLATE` — JS for ARIA-tree extraction, placeholders `__SELECTOR__` and `__REF_START__`
- All commands except `doctor` now use `merge_into_if_verbose`

## How to apply

- When adding new commands, use `merge_into_if_verbose` not `merge_into`
- Default meta should only include per-request semantics (selector, depth, settle_method)
- `host`/`port` go in `meta.connection` (verbose only), not in base meta
- When changing dom/snapshot output shape, the `--format html` path is the legacy escape hatch
