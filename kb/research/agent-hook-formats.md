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
codex_loader: >-
  Actual final installer output loaded by isolated Codex0.153.4 app-server
  hooks/list 2026-09-12 with CODEX_HOME=temporary/active-codex distinct from
  HOME=temporary/home and HOME/.codex explicitly disabled. One user SessionStart command,
  enabled=true, trustStatus=untrusted, isManaged=false, no warnings/errors. initialize
  also confirmed active customhome. Four original/default/custom config and hook
  file hashes unchanged. No global config/trust changes, hook execution or model turn
  requested; app-server stopped/reaped. Evidence
  iter254/review-repair-1/loader-result.json, loader-ownership.json, loader-file-verification.json.
codex_shell: >-
  Windows uses an absolute OS-discovered system PowerShell executable, safely
  cmd-quoted. GetSystemDirectoryW avoids environment and cwd/PATH discovery. The
  ff-rdp executable remains separately UTF8-base64 DATA inside an outer UTF16LE-base64
  PowerShell command, preserving literal smart quotes/metacharacters.
  ErrorActionPreference Stop preserves visible missing-command failure and child0/23. See
  review_repair_2_shell for API/source evidence, system-path refusal cases and the native
  decoy/mutation regression. Native exact-head Windows CI for repair2 remains
  required; old repair1 Windows passes do not certify this product change.
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
codex_home: "Verified 2026-09-12 from saved hooks/config docs first, then official https://learn.chatgpt.com/docs/config-file/environment-variables.md: CODEX_HOME is the active state/config root; default ~/.codex. The installer resolves nonempty CODEX_HOME for hooks.json and config.toml, falls back through HOME/USERPROFILE when empty/unset, and never writes the inactive default home. Isolated actual Codex0.153.4 app-server initialize and hooks/list confirmed custom-home selection on the final installer output. Evidence iter254/review-repair-1/codex-environment-variables.md and loader-result.json."
review_repair_1_probe_portability: "2026-09-12 test-only follow-up within repair1: Windows PowerShell5.1 powershell.exe requires -Version <version>, unlike pwsh version display. Replaced only the cfg(test) availability probe with separate argv -NoLogo -NoProfile -NonInteractive -Command exit 0, supported by both. Source: https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.core/about/about_powershell_exe?view=powershell-5.1 . Actual local PowerShell regression passed, preserving mandatory Windows execution, optional missing-pwsh Unix distinction, Unicode/injection and native exit0/23/missing-command controls. Ordered stable/fmt/strictclippy/workspace gates rerun and pass. Product prefix and all other previous manifest files are byte-identical to frozen5d7c0aa0cc85737465bd8311af172a3317f9e7cb; scope-proof.json/test-only.patch retain exact proof. Per supervisor,338 final-product sweep330pass+8fail and actual28.41s browser dogfood are REUSED, not rerun or replaced. Eight applicable static xtask gates pass. Dogfood static lint passed, but its complete gate REFUSED exit1 because iter-* requires FF_RDP_LIVE_TESTS=1; this invocation is not a green dogfood gate. Per the test-only reuse instruction, prior actual successful28.41s gate is retained with unchanged product/script proof; no new browser run. This explicit refusal remains in xtask-results.json. Native exact-head Windows CI is still required; no repaired Windows execution/failure is claimed. Prior267 missing failed-occurrence timing and all203/262 observations unchanged. No original tasks/ACs/status or pending Outcome/counter exception changed."
review_repair_2_shell: "2026-09-13 review repair2: refetched pinned Codex rust-v0.153.4 command_runner.rs confirms current_dir(cwd), COMSPEC /C fallback cmd.exe, raw_arg of a double-quoted command. Source https://github.com/openai/codex/blob/rust-v0.153.4/codex-rs/hooks/src/engine/command_runner.rs . Microsoft https://learn.microsoft.com/en-us/windows-server/administration/windows-commands/path documents cwd before PATH; https://learn.microsoft.com/en-us/windows/win32/api/sysinfoapi/nf-sysinfoapi-getsystemdirectoryw documents the OS system-directory API and buffer/error contract. Installer now resolves GetSystemDirectoryW and WindowsPowerShell/v1.0/powershell.exe, validates an existing absolute file, quotes it, and refuses failed discovery, invalid Unicode or cmd-expanding system-path characters (percent, exclamation, double quote/control). No C-drive hardcoding or SystemRoot/WINDIR/PATH discovery; existing narrow audited FFI policy unchanged. Assumes the OS system directory and system executable are administrator-controlled, and Codex session COMSPEC is trusted; protecting a malicious shell chosen by Codex is outside this hook generator. WOW64 follows Windows system-directory redirection to installed matching PowerShell. ff-rdp executable data remains separately encoded, including literal Unicode/metacharacters. Native Windows decoy regression includes a deliberate bare-shell mutation (must execute decoy/97), trusted child0/23, missing1, and absence of decoy marker after repair; exact-head Windows CI pending, macOS does not certify cmd execution."
---
