# ff-rdp — agent guide

ff-rdp is an LLM agent's eyes on a page: a CLI over Firefox's Remote Debugging Protocol to
navigate and then inspect — text, DOM, computed styles and the cascade, console, network,
screenshots, accessibility, performance, `eval`. It is not a browser-automation driver; minimal
interaction (`click`, `type`, `scroll`) exists only to reach the state you want to inspect.

Read before working:
- `CLAUDE.md` — gates, code patterns, live-test and PR rules, merge authorization.
- `.claude/CLAUDE.md` — knowledge-base layout (`./kb`) and test-fixture recording.
- `kb/research/step-back-2026-10-02.md` — why the process is what it is.

Use the `hyalo` CLI for all supported markdown knowledge-base operations (`hyalo --help`;
`--format text` for compact output). Edit body prose directly where hyalo has no command.
