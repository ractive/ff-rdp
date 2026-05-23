---
title: "hyalo bug: backlinks drops bare-basename wikilinks"
type: bug-report
date: 2026-05-23
status: fixed
fixed_in: 0.15.0 (rebuilt 2026-05-23 15:32; no version bump)
tool: hyalo
version_observed: 0.15.0
tags: [hyalo, bug, fixed, wikilinks, kb-tooling]
---

> **Status: fixed** as of `~/.cargo/bin/hyalo` rebuilt at 2026-05-23 15:32. The reproducer below now reports all three backlinks (lines 6, 7, 8). Keeping this document as a regression-test reference: if the 3-line reproducer ever returns fewer than three entries again, the bug has come back.



# hyalo bug: `backlinks` drops bare-basename wikilinks

`hyalo backlinks <target.md>` silently omits incoming wikilinks of the form `[[basename]]` (with or without a display label). `hyalo find --fields links` resolves the same wikilinks correctly to the target — so the two commands disagree on the link graph. Confirmed against `hyalo 0.15.0` and rebuilt-on-disk binary `/Users/james/.cargo/bin/hyalo` (mtime 2026-05-23 15:15).

## Reproducer

From the repo root (vault dir is `kb`):

```sh
cat > kb/_quirk-test.md <<'EOF'
---
title: Hyalo Quirk Test
type: scratch
---

Line 6: [[reflow]] (short form, no label)
Line 7: [[rdp/resources/reflow|reflow]] (long form with label)
Line 8: [[reflow|reflow]] (short form WITH label)
EOF

# 1. find resolves all 3 links to the same path:
hyalo find --file _quirk-test.md --fields links --format json \
  --jq '.results[0].links'

# 2. backlinks reports only the long form:
hyalo backlinks kb/rdp/resources/reflow.md --format json \
  --jq '.results.backlinks | map(select(.source == "_quirk-test.md"))'

rm kb/_quirk-test.md
```

## Observed output

`find --fields links` (all three resolve to the same `path`):

```json
[
  {"label": null,     "path": "rdp/resources/reflow.md", "target": "reflow"},
  {"label": "reflow", "path": "rdp/resources/reflow.md", "target": "rdp/resources/reflow"},
  {"label": "reflow", "path": "rdp/resources/reflow.md", "target": "reflow"}
]
```

`backlinks` (only line 7 — both short-form lines 6 and 8 dropped):

```json
[
  { "source": "_quirk-test.md", "line": 7, "label": "reflow",
    "target": "rdp/resources/reflow" }
]
```

## Expected

Three backlinks (lines 6, 7, 8). `find --fields links` and `backlinks` must share a target resolver — if `find` resolves `[[reflow]]` to `rdp/resources/reflow.md`, `backlinks` for that file must include the source.

## Likely cause

`backlinks` builds its reverse-edge graph keyed on the raw wikilink `target` token (e.g. `"reflow"`) rather than the resolved vault path (`"rdp/resources/reflow.md"`). When `backlinks` is queried for a vault path, it never finds the bare-basename edge. `find --fields links` already does the basename-search correctly — `backlinks` should funnel through the same resolver before insertion.

## Why it matters

- Short-form `[[basename]]` wikilinks are the Obsidian/Zettelkasten default.
- `find --orphan` inherits the same bad graph, so vaults using short-form links see false orphans.
- The bug is silent — both commands return valid-looking data, they just disagree.

## Tested paths (all reproduce)

| Path | Result (lines reported out of 6, 7, 8) |
|---|---|
| `backlinks` direct scan | only line 7 |
| `backlinks --index` (no index → falls back to scan) | only line 7 |
| `backlinks --index` with freshly-built `.hyalo-index` (216 files) | only line 7 |

## Workaround

Rewrite short-form wikilinks to path form: `[[basename]]` → `[[full/vault/path/basename|basename]]`. `hyalo links fix --apply` automates most of this, but produces ambiguous collapses when the same basename exists in two folders (e.g. `transport.md` lives in both `rdp/protocol/` and `rdp/client/`) — those need manual disambiguation.

## Suggested test for the fix

A unit test that mirrors the reproducer:

1. Create a source file with three wikilink forms (`[[X]]`, `[[X|label]]`, `[[path/to/X|label]]`), all pointing at the same target.
2. Assert `backlinks` returns three entries for the target file.

## Context

Surfaced while running `/hyalo-tidy` on the freshly-created `kb/rdp/` wiki (commit `5013bb6`). Tidy initially reported 11 "orphans" inside `rdp/`; investigation showed every claimed orphan had at least one short-form incoming wikilink that `backlinks` was silently dropping. After rewriting every short-form wikilink to path form, the orphan count went to zero — masking the real fix.
