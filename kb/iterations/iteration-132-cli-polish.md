---
branch: iter-132/cli-polish
date: 2026-07-19
depends_on: []
dogfood_path: |
  ff-rdp launch --headless --auto-consent
  ff-rdp tabs --jq '['
  # → clean parse-error message with position, no Rust Debug dump, exit 1
  ff-rdp eval 'await Promise.resolve(41) + 1'
  # → 42 (top-level await works)
  ff-rdp navigate https://example.com >/dev/null
  ff-rdp eval 'document.querySelector("h1").textContent'
first_call_sites: []
status: planned
---

# Iteration 132: CLI polish — error surfaces, live DOM values, ergonomics, housekeeping

Grab-bag of small confirmed issues from [[dogfooding-session-61]]/[[dogfooding-session-62]],
bundled into one PR cycle on purpose (per-iteration overhead is high; none of these
justifies its own gate run).

## Findings driving this iteration

1. **Malformed `--jq` leaks a raw Rust Debug struct** (s61 #13, COSMETIC, still broken):
   `tabs --jq '['` → `{"error":"jq parse error: Lex(\n [\n (\n Delim(\"[\"…"}` — the
   `Lex(...)` Debug dump is embedded in the JSON error envelope. Exit 1 is correct.
2. **`dom` `attrs.value` is static, not live** (s61 #10, MINOR, still broken): after
   `.value="42"` the envelope still reports the HTML attribute (`"0"`), with no live
   value field — misleading for form debugging.
3. **Top-level `await` in `eval` throws SyntaxError** (dogfood-62 friction): despite
   `eval_path:page-await`, `eval 'await fetch(...)'` fails with "await only valid in
   async fn"; agents reach for top-level await naturally. `.then()` works, so the
   await plumbing exists — the script just needs an async-IIFE wrap when it contains
   top-level await.
4. **Natural-but-wrong flag guesses get unhelpful errors** (dogfood-62 friction):
   `scroll --bottom` and `dom --stats` fail where `scroll bottom` / `dom stats`
   (subcommands) are correct; worse, the `dom --stats` error tip suggests `--attrs`,
   which misleads.
5. **`~/.ff-rdp/` accumulates stale zero-byte `daemon.*.spawn.lock` files** (dogfood-62
   #9, housekeeping): ~50 locks from dead pids, never cleaned, growing unbounded.

## Themes

- **A — clean error surfaces.** Human-readable jq parse errors (position + snippet,
  no `Debug` formatting) across all commands.
- **B — live values in `dom`.** For form elements, report the live `value` property
  alongside `attrs.value` (distinct field, e.g. `value`), so live state is visible
  without an eval round-trip.
- **C — eval accepts top-level await.** Detect top-level `await` and wrap the script
  in an async IIFE before evaluation; result and `eval_path` semantics unchanged.
- **D — flag-vs-subcommand ergonomics.** `scroll --bottom`, `dom --stats` (and
  siblings) produce errors suggesting the correct subcommand; fix the misleading
  `--attrs` tip.
- **E — spawn-lock GC.** Daemon startup removes stale `daemon.*.spawn.lock` files
  whose pid is dead; bound the directory's growth.

## Tasks

- [ ] A: intercept the jq crate's parse error, render position + input snippet;
      audit for other `{:?}` leaks on user-facing error paths.
- [ ] B: live `value` property fetch in the dom inspector path for
      input/textarea/select; document the attrs-vs-value distinction in help.
- [ ] C: top-level-await detection + async-IIFE wrap in eval script preparation
      (all entry points: arg, --file, --stdin).
- [ ] D: clap error customization for the known flag-vs-subcommand traps; correct
      the `dom --stats` tip.
- [ ] E: stale-lock GC on daemon spawn; unit-test with a fabricated dead-pid lock.

## Acceptance Criteria [0/5]

<!-- Each AC names a live test + asserted post-condition, per CLAUDE.md convention. -->

- [ ] unit_jq_parse_error_clean: malformed `--jq` yields an error string containing
      the failing position and NO `Lex(`/`Delim(` Debug fragments; e2e asserts exit 1
      and the clean message on stdout envelope.
- [ ] live_132_dom_live_value: fixture input with attribute value "0"; after
      `eval '...value="42"'`, `dom '#el'` reports `value:"42"` AND `attrs.value:"0"`.
- [ ] live_132_eval_top_level_await: `eval 'await Promise.resolve(41) + 1'` → results
      42, exit 0, on all three input paths (arg, --file, --stdin).
- [ ] e2e_flag_subcommand_hints: `scroll --bottom` and `dom --stats` stderr suggests
      `scroll bottom` / `dom stats` respectively; no `--attrs` tip for `dom --stats`.
- [ ] unit_spawn_lock_gc: daemon spawn path removes a stale lock with a dead pid and
      keeps a live-pid lock.

## Notes

Sibling plans from the same findings batch: [[iteration-128-network-hint-always-present]],
[[iteration-129-consent-and-cross-origin-frames]], [[iteration-130-navigation-truthfulness]],
[[iteration-131-measurement-honesty]].
Not planned (accepted platform limits / by design): `wait --timeout` deprecation
(s61 #12), Firefox LCP unavailability (documented note), multi-instance artifacts
(s61, structurally resolved by iter-123).
