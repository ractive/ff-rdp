# Reproduce the ff-rdp browser benchmark

The upstream source is [kunchenguid/axi at
d28c5e79aa7ee7a59a386fc34125f8cd1470fbeb](https://github.com/kunchenguid/axi/tree/d28c5e79aa7ee7a59a386fc34125f8cd1470fbeb).
The original `bench-browser/src/runner.ts` remains unchanged: SHA-256
`11659d64e71fa116744f6b837d0b8b246c8623eb0f3cf796a2342a7567a24a8b`.
The patch adds the historical condition and safe ff-rdp lifecycle handling. No
upstream TypeScript is checked into this repository.

Use macOS or Linux with Bash, Git, Rust, jq, lsof, shasum, Firefox, Node >=20,
pnpm, and authenticated Claude Code. The pinned upstream workspace instructions
use `pnpm install`; this harness adds `--frozen-lockfile`. The verified environment
on 2026-09-13 uses Node 24.19.0 and a pnpm 11.24.0 launcher; the upstream
package-manager pin selects pnpm 11.1.1 inside its workspace. Both are recorded.
Windows users need a Unix host
for this shell harness; this does not change ff-rdp's Windows support.

From a clean product checkout (the output directory must be new):

```sh
tools/axi-bench/run.sh --output /tmp/ff-rdp-baseline-20260913
```

This is a paid matrix: **14 task definitions × 3 repetitions = 42 runs**, only
the ff-rdp condition. Do not rerun poor results to improve the numbers and do not
rerun the historical axi baseline. Task ordering is randomized upstream.
`--task wikipedia_link_follow,wikipedia_infobox_hop --repeat 3` selects the six
iteration 255 runs. Label them with `--label iteration-255`. A setup-only run uses
`--prepare-only`; a bounded, separately labeled smoke uses
`--task read_static_page --repeat 1 --label harness-smoke` and is not acceptance
of the final matrix. Preparation, smoke, and measurement need separate outputs.

## Exported harness, different product revision

Iteration 256 owns this harness. Its prerequisite checkpoint is exported before
iteration 255; the final iteration 256 matrix follows the merged 255 product.
An export can contain just this directory. Supply the exact **harness** checkpoint
SHA separately from the clean **product** checkout and its expected full HEAD:

```sh
/absolute/export/tools/axi-bench/run.sh \
  --harness-revision HARNESS_CHECKPOINT_FULL_SHA \
  --source-root /absolute/iteration-255-checkout \
  --revision PRODUCT_FULL_SHA \
  --task wikipedia_link_follow,wikipedia_infobox_hop --repeat 3 \
  --label iteration-255 --output /tmp/iteration-255-measurement
```

The supervisor must export the named checkpoint faithfully; every harness file's
hash is also saved to `harness.sha256`. The script rejects a dirty product or a
different HEAD, builds that product using its `dogfood-lib.sh` into a fresh private
Rust target, and puts that binary first on PATH. A preinstalled `ff-rdp` is never
used for the build or lifecycle. Provenance records both revisions, absolute source
and binary paths, observed binary version and SHA-256, model, CLI and runtime
versions, prompt hash, matrix denominator and label. Each agent artifact has its
own provenance copy; every agent/judge invocation also has an audit copy and argv.
The product is checked again immediately before launching the matrix. Keep the
checkout exclusive throughout the run.

## Prompts, models, and costs

The unchanged ff-rdp `--append-system-prompt` paragraph is:

```text
# Tools

You have the `ff-rdp` CLI installed for browser automation.
Use it for all browsing tasks. Do NOT use curl, wget, or WebFetch.

Run `ff-rdp --help` for available commands and usage.
```

Including the YAML block scalar's trailing newline, SHA-256 is
`758dfb37452f8099cfb46460ee417ccc73839cf0ee3553f7da0558229be49c8b`.
The historical axi paragraph is identical except for the CLI name:

```text
# Tools

You have the `chrome-devtools-axi` CLI installed for browser automation.
Use it for all browsing tasks. Do NOT use curl, wget, or WebFetch.

Run `chrome-devtools-axi --help` for available commands and usage.
```

Its SHA-256 is `576f9835b62ad24111d2d6cbc78bd1b58ac2d1e6546570e90a7c0a0f0578e12a`.
These append paragraphs supplement Claude Code's default system prompt. Historical
CLI **2.1.241** differs from the currently verified **2.1.259**, so exact default
prompt equivalence cannot be claimed. Agent and judge request
`claude-sonnet-4-6`; no model fallback is allowed. Actual `modelUsage` can include
auxiliary Haiku usage and must be reported honestly.

`claude-record.sh` records raw responses, original argv, task prompt hash, exit
status, actual `modelUsage` and list cost. The judge's output format alone changes
from text to JSON to expose usage, then its result is converted back to text for
the unchanged grader. Keep judge cost separate from upstream's agent-cost table;
use the invocation records for total list cost. This is list pricing, not a claim
about subscription billing. Upstream can swallow Claude failures and still exit
zero: inspect **all invocation exits/error fields**, grades, and result counts.
An exit-zero matrix alone is not a successful benchmark.

Agent output is forwarded incrementally through `tee` while the exact bytes are
saved in `stdout.jsonl`; partial trajectories remain available after a timeout.
The recorder creates a private process group for Claude and its ordinary child
processes. On TERM/INT or a recorder failure it terminates that group, escalates
to KILL after two seconds if necessary, and waits for its direct child. Normal
completion also removes leftover descendants. `child.pgid`, `child.exit`,
`interrupted.exit`, `stream.exit` (agents), and `cancellation.log` when needed
record that lifecycle; interrupted wrapper exits are 143 (TERM) and 130 (INT).
The judge retains its separate JSON-to-text interface. A process that deliberately
leaves the owned process group is outside that ownership boundary; pipe draining
is bounded so such a process cannot hang the recorder's cancellation reporting.

The stale inherited `ANTHROPIC_API_KEY` is omitted only for the child process and
its descendants, retaining the existing cached account. No login, account switch,
global environment, settings or model changes occur. `--setting-sources ""`
disables baseline hooks/settings; a later ambient payload versus installed-hook
treatment must be separate and labeled explicitly. It is not implemented here.

## Ownership and evidence

Port 6000 must be free. The lifecycle launches a private raw headless Firefox,
checks that its recorded PID owns that listener, and uses a private `FF_RDP_HOME`
and unique product binary for daemon ownership. Cleanup checks PID start time and
command identity before each signal. It never calls global `daemon stop`, `pkill`,
or upstream's broad orphaned-Chrome cleanup. Each task gets a fresh profile/state.
An occupied port or an intruder PID is refused and left alone.

Results, invocation logs, lifecycle identities, process cleanup logs and provenance
are copied into the output directory before temporary source/target removal.
Require `cleanup.exit` to be zero independently of `matrix.exit`; the wrapper
preserves the matrix child's exit status even if cleanup reports an anomaly.
Inspect interrupted runs manually before reusing port 6000. The harness preserves
temporary directories if cleanup refuses ownership so evidence is recoverable.
Failed lifecycle/results copies or an unwritable cleanup verdict also retain
the temporary work and target. The diagnostic names both recovery directories;
copy failures contribute to `cleanup.exit` even when the matrix itself exited 0.

Run `bash tools/axi-bench/test.sh` for fixture ownership/status, streaming,
cancellation and evidence-retention checks, and
`bash tools/axi-bench/test-mutations.sh` to verify that removing each repaired
protection triggers its intended fixture assertion. These tests need `rg` and
retain their private stream/copy artifacts for inspection. Live lifecycle
verification and a paid smoke are separate, explicitly labeled evidence; neither
completes iteration 256's clean-checkout comparison acceptance criterion.
