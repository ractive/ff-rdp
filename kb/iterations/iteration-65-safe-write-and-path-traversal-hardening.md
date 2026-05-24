---
title: "Iteration 65: Safe-write helper + path-traversal hardening"
type: iteration
date: 2026-05-24
status: completed
branch: iter-65/safe-write
depends_on:
  - iteration-63-daemon-lockrecover-and-quick-sec-fixes
first_call_sites:
  - primitive: ff_rdp_cli::util::safe_write
    site: >-
      crates/ff-rdp-cli/src/commands/screenshot.rs::resolve_output_path (writes
      through safe_write instead of fs::write)
  - primitive: ff_rdp_cli::util::safe_create
    site: >-
      crates/ff-rdp-cli/src/commands/auto_consent.rs::install (opens the XPI
      destination with O_NOFOLLOW|O_EXCL)
dogfood_path: |
  # 1. Symlink at destination is rejected (not followed).
  ln -s /tmp/innocent.png /tmp/screenshot.png
  ff-rdp screenshot --out /tmp/screenshot.png https://example.com
  # Expected: exits non-zero, "refusing to write through symlink"
  rm /tmp/screenshot.png
  
  # 2. Path traversal is rejected when --output-root is set.
  ff-rdp index --output-root /tmp/maps --output ../etc/page-map.json https://example.com
  # Expected: exits non-zero, "output path escapes --output-root"
  
  # 3. Normal case still works.
  ff-rdp screenshot --out /tmp/ok.png https://example.com
  test -f /tmp/ok.png
tags:
  - iteration
  - security
---

# Iteration 65: Safe-write helper + path-traversal hardening

`fs::write` follows symlinks; a pre-created symlink at the destination
redirects the write. The screenshot writer (`--out`), page-map writer
(`--output`), `install_skill`, the recorder, and `auto_consent::install`
all accept caller-supplied paths and write through them without
`O_NOFOLLOW` or any traversal containment. Add a `safe_write` /
`safe_create` helper, route every operator-path write through it, and add
an optional `--output-root` containment flag.

## Themes

- **A — `safe_write` / `safe_create` helper.** A single helper that opens
  destinations with `O_NOFOLLOW | O_CREAT | O_EXCL` (Unix) and the
  equivalent flags on Windows, and rejects writes that would follow a
  symlink or overwrite a different file type.
- **B — Wire the helper into every operator-path writer.** Screenshot,
  page-map, install_skill, recorder, auto_consent.
- **C — `--output-root` containment.** Optional CLI flag that constrains
  resolved output paths to a given ancestor after canonicalization; rejects
  `..` traversal.

## Tasks

### A. Helper
- [x] Add `crates/ff-rdp-cli/src/util/safe_io.rs` exporting `pub fn safe_write(path, bytes) -> Result<()>` and `pub fn safe_create(path) -> Result<File>`.
- [x] Unix: use `OpenOptions::custom_flags(libc::O_NOFOLLOW)`. Windows: rely on `CreateFile`'s default behaviour (no symlink resolution by default) plus an explicit `is_symlink` precheck on the parent.
- [x] Unit-test the helper against a fixture symlink → expect typed error `SafeIoError::SymlinkRefused`.

### B. Call-site migration
- [x] `crates/ff-rdp-cli/src/commands/screenshot.rs:290-301` — replace `fs::write` with `safe_write`.
- [x] `crates/ff-rdp-cli/src/commands/index.rs:696-731` — same; the page-map writer currently uses `write_atomic` which also follows symlinks at rename time.
- [x] `crates/ff-rdp-cli/src/commands/install_skill.rs` (audit for `fs::write` / `fs::copy` follow-the-symlink calls). — paths derived from install-root, not caller input; noted in safe_io.rs doc comment.
- [x] `crates/ff-rdp-cli/src/script/recorder.rs` (audit for writers that touch caller-supplied paths). — XDG state paths only; noted in safe_io.rs doc comment.
- [x] `crates/ff-rdp-cli/src/commands/auto_consent.rs:33-62` — switch the XPI write to `safe_create` (closes finding F-9; complements iter-64).

### C. `--output-root` containment
- [x] Add a `--output-root <dir>` flag on commands that take output paths (screenshot, index, record). — added to screenshot and index; record writes to XDG state, not caller paths.
- [x] When set, the helper canonicalizes the destination and rejects writes whose canonical form is not a descendant.
- [x] Test the rejection path with a `../` traversal attempt.

## Acceptance Criteria [6/6]

- [x] `safe_write_rejects_symlink`: writing through a pre-existing symlink returns `Err(SafeIoError::SymlinkRefused)` — test `util::safe_io::tests::safe_write_rejects_symlink` passes.
- [x] `safe_write_succeeds_on_regular_file`: ordinary write path still works on a fresh file and on overwriting a regular file — test `util::safe_io::tests::safe_write_succeeds_on_regular_file` passes.
- [x] `screenshot_safe_write_wired`: `commands/screenshot.rs` uses `safe_write` (no `fs::write` left in the module) — confirmed by grep; test `safe_write_succeeds_on_regular_file` covers the write path.
- [x] `index_safe_write_wired`: `commands/index.rs::write_page_map` uses `safe_write` — `write_page_map` (renamed from `write_page_map_atomic` after review since the rename-based atomicity was dropped) calls `crate::util::safe_io::safe_write`.
- [x] `output_root_rejects_traversal`: with `--output-root /tmp/maps`, `--output ../etc/foo.json` exits non-zero with a typed error — unit test `util::safe_io::tests::ensure_within_root_rejects_traversal` passes.
- [x] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q` clean — all three gates passed.

## Design notes

`O_NOFOLLOW` on Linux refuses to open a symlink as the final path component
but does NOT prevent intermediate symlinks. The realistic threat is a
pre-created symlink AT the destination, which `O_NOFOLLOW` fully handles.
For deeper protection use `openat2(RESOLVE_NO_SYMLINKS)` on Linux ≥ 5.6,
behind a feature flag — out of scope here.

Windows: `CreateFile` does not follow symlinks by default for `OPEN_EXISTING`
mode, but for `CREATE_NEW` / `CREATE_ALWAYS` the behaviour depends on the
volume. The portable approach: precheck `path.is_symlink()` before opening
and refuse if true. Race-y but acceptable for our threat model.

`--output-root` is optional, opt-in. Default behaviour stays the same so
existing scripts don't break.

## Out of scope

- Linux `openat2` deep traversal protection.
- Sandboxing the spawned Firefox's file access (would need profile-level
  policies; out of scope).

## References

- [[iteration-63-daemon-lockrecover-and-quick-sec-fixes]]
- [[iteration-64-xpi-integrity]]
- Security review report (2026-05-24), findings F-4, F-9
