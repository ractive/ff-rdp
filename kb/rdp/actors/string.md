---
type: rdp-note
tags:
- rdp
- firefox-server
- actor
- string
date: 2026-05-24
firefox_files:
- devtools/shared/specs/string.js
- devtools/server/actors/string.js
title: LongStringActor
---

# LongStringActor

Represents a long string value on the server. Strings longer than a threshold
(typically 32 KB) are not sent inline in a grip — instead a `LongStringActor`
reference is sent and the client fetches substrings on demand via `substring()`.

## Firefox references

| File | Lines | Purpose |
|------|-------|---------|
| `devtools/shared/specs/string.js` | 1-85 | Protocol spec — substring, release methods |
| `devtools/server/actors/string.js` | 1-45 | Server implementation |

## Key methods (from spec)

- `substring(start, end)` — fetch a character range from the remote string.
- `release()` — release the actor (release method).

## Status

Stub — backfilled in iter-73; expand on next touch.
