---
type: research
title: Agent hook formats
date: 2026-09-12
status: completed
codex_version: >-
  0.153.4 (installed CLI, SHA256
  b973d440acac501fd2594a43e7ca9ce41e0a65b9dfb28d0d7a7837c99e1261e3)
codex_source: >-
  https://learn.chatgpt.com/docs/hooks (recorded 2026-09-09, unchanged on
  2026-09-12); https://developers.openai.com/codex/config-schema.json (hook definitions
  unchanged on 2026-09-12)
codex_shape: "{\"hooks\": {\"SessionStart\": [{\"hooks\": [{\"type\": \"command\", \"command\": \"ff-rdp home --hook\"}], \"ff_rdp_managed\": true}]}}"
codex_gate: >-
  Codex defaults hooks ON. ff-rdp conservatively requires explicit boolean
  features.hooks=true in user config.toml, parsed as TOML; it never edits config or
  trust. Missing, false and malformed gates refuse with no writes. Uninstall bypasses
  this installer prerequisite. The check is not a guarantee of effective
  enterprise/project policy.
codex_trust: >-
  New/changed hooks require review and trust through /hooks. All active sources
  accumulate; users should inspect duplicate commands. ff_rdp_managed is only ff-rdp
  ownership metadata, unrelated to Codex managed policy or trust.
codex_loader: "Actual installer output loaded by isolated Codex 0.153.4 app-server hooks/list on 2026-09-12: one user SessionStart command, enabled=true, trustStatus=untrusted, isManaged=false, no warnings/errors. HOME/USERPROFILE/CODEX_HOME were temporary. No trust changed and no model turn or hook execution was requested. Source and installer file bytes remained local to the temporary home; evidence iter254/loader-result.json."
codex_shell: "Pinned upstream tag rust-v0.153.4 codex-rs/hooks/src/engine/command_runner.rs: Unix SHELL -lc, fallback /bin/sh; Windows COMSPEC /C, fallback cmd.exe. Codex target single-quotes Unix paths; Windows transports a literal PowerShell command in UTF-16LE base64 to avoid cmd percent expansion. Native argv/sentinel test is platform-gated; macOS run is not Windows runtime evidence."
opencode_version: >-
  1.3.13 checkout b5b5f7e0190cdd5272b6d2aeb3d4589a822675a6, clean, no installed
  opencode executable on PATH
opencode_source: >-
  https://opencode.ai/docs/plugins/;
  https://github.com/anomalyco/opencode/blob/b5b5f7e0190cdd5272b6d2aeb3d4589a822675a6/packages/plugin/src/index.ts;
  packages/opencode/src/config/config.ts discovers {plugin,plugins}/*.{ts,js}
opencode_contract: "Plugin = (input: PluginInput, options?: PluginOptions) => Promise<Hooks>; Hooks.event receives {event: Event}; session.created event is documented. Requires JS/TS modules or npm plugins; no command-only session-start config established. Theme C is obsolete under the explicit Rust-only rule; --opencode retains its refusal. No installed-runtime validation claimed."
fixtures: >-
  crates/ff-rdp-cli/tests/fixtures/codex-hooks-documented.json is the exact
  Config shape JSON code block from recorded official hooks docs.
  codex-hooks-schema.json extracts HooksToml plus transitive definitions from the recorded official input
  schema; not app-server output. opencode-plugin-contract.txt is the verbatim
  pinned plugin type source; evidence for refusal, not shipped executable plugin code.
  Firefox fixture recording rules concern RDP packet fixtures; these are the
  plan-mandated external agent format records.
editing_note: >-
  Hyalo new created this research scaffold; findings are frontmatter because
  Hyalo has no arbitrary body-edit API. Original iteration task and acceptance wording
  is preserved.
---
