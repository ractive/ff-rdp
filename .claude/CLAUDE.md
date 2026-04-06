# Documentation

Keep all documentation in `./kb` as `*.md` markdown files with YAML frontmatter (text, numbers, checkboxes, dates, lists). Use it as your second brain:
- Research outcomes → `research/`
- Design decisions → `decision-log.md`
- Iteration plans → `iterations/iteration-NN-slug.md` (one file per iteration, markdown task lists for steps/tasks/ACs)

Organize in subfolders. Use `[[wikilinks]]` for cross-references. Keep Obsidian-compatible.

<!-- hyalo:start -->
Use `hyalo` CLI (not Read/Grep/Glob) for all markdown knowledgebase operations.
Examples: `hyalo find --property status=planned --format text`, `hyalo find "search text"`, `hyalo find --property 'title~=pattern'`.
Run `hyalo --help` for usage. Use `--format text` for compact LLM-friendly output.
<!-- hyalo:end -->

# Test Fixtures

All e2e test fixtures (`tests/fixtures/*.json`) **must** be recorded from a real Firefox instance — never hand-craft them.

## Recording workflow
1. Launch headless Firefox: `firefox -no-remote -profile /tmp/ff-rdp-test-profile --start-debugger-server 6000 --headless`
2. Record all fixtures in one command:
   ```sh
   FF_RDP_LIVE_TESTS_RECORD=1 cargo test -p ff-rdp-core --test live_record_fixtures -- --ignored
   ```
3. Fixtures are automatically normalized (actor IDs `conn\d+` → `conn0`) and written to both `ff-rdp-core/tests/fixtures/` and `ff-rdp-cli/tests/fixtures/`
4. To add a new fixture: add a live test in `crates/ff-rdp-core/tests/live_record_fixtures.rs` using `save_cli_fixture()`/`save_core_fixture()`
