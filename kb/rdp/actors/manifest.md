---
type: rdp-note
tags:
- rdp
- firefox-server
- actor
- manifest
- pwa
- manifest-command
date: 2026-07-09
firefox_files:
- devtools/shared/specs/manifest.js
- devtools/server/actors/manifest.js
title: ManifestActor
---

# ManifestActor (typeName `"manifest"`)

Fetches and validates the current document's **Web App Manifest**. Consumed by
the `ff-rdp manifest` command (iter-104) — the PWA-readiness audit primitive.

## Acquisition

The `manifestActor` id is exposed on the **target frame** returned by
`getTarget` (alongside `consoleActor`, `inspectorActor`, …; see
[[rdp/actors/targets/window-global-target]]). Firefox creates it **lazily** on
first access. ff-rdp reads it from
[[../../../crates/ff-rdp-core/src/actors/tab.rs|`TargetInfo::manifest_actor`]]
and wraps it in `ManifestFront`.

Older Firefox builds that predate the manifest actor omit the field — the CLI
then returns a capability error (`no manifest actor available`), distinct from
the structured "page has no manifest" result below.

## `fetchCanonicalManifest`

Spec: `devtools/shared/specs/manifest.js:14-17`. One call runs the WHATWG
"obtain a manifest" algorithm and returns the parsed manifest **plus**
conformance errors:

```
{ manifest: { url: <resolved manifest URL>,
              values: <parsed manifest object | null>,
              errors: [ <conformance error objects> ] } }
```

- `values` carries the parsed manifest (`name`, `short_name`, `start_url`,
  `display`, `icons`, `theme_color`, …), or `null` when the page links no
  `<link rel="manifest">`.
- `errors` lists conformance problems the manifest processor found.
- `url` is the resolved manifest URL (absolute).

`ManifestFront::fetch_canonical_manifest` projects this into `CanonicalManifest`
(`manifest: Option<Value>`, `url: Option<String>`, `errors: Vec<Value>`),
tolerating the older shape where the parsed values live under `manifest`
instead of `values`.

## No-manifest is not an error

A page that links no manifest yields `manifest: values = null`. The `ff-rdp
manifest` command surfaces this as a **structured, exit-0** result —
`results.manifest = null` with a `results.reason` string — so scripts branch on
presence without parsing error output. Only a missing manifest **actor** (old
Firefox) or a transport failure produces a non-zero exit.

## Lifecycle

- Owned by the tab target; invalidated when the target is destroyed (e.g. on
  navigation). ff-rdp re-reads the actor id from a fresh `getTarget`.
- No events.
