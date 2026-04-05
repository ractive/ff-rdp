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
