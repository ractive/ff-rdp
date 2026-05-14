//! `ff-rdp install-skill` — install bundled Claude Code skill files to the filesystem.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Context as _;
use include_dir::{Dir, DirEntry};
use serde_json::{Value, json};

use crate::cli::args::{Cli, InstallSkillArgs, SkillScope};
use crate::error::AppError;
use crate::output;
use crate::output_pipeline::OutputPipeline;

// ---------------------------------------------------------------------------
// Embedded skill registry
// ---------------------------------------------------------------------------

static FF_RDP_DEBUG_DIR: include_dir::Dir<'_> =
    include_dir::include_dir!("$CARGO_MANIFEST_DIR/skills/ff-rdp-debug");

struct SkillDef {
    name: &'static str,
    dir: &'static Dir<'static>,
}

static REGISTRY: &[SkillDef] = &[SkillDef {
    name: "ff-rdp-debug",
    dir: &FF_RDP_DEBUG_DIR,
}];

fn registry() -> &'static [SkillDef] {
    REGISTRY
}

// ---------------------------------------------------------------------------
// Managed-by header helpers
// ---------------------------------------------------------------------------

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn managed_by_header(skill_name: &str, ext: &str) -> Option<String> {
    let tag = format!("managed-by: ff-rdp v{VERSION} skill={skill_name}");
    match ext {
        "md" | "html" | "htm" => Some(format!("<!-- {tag} -->\n")),
        "json" => None, // handled specially — inserted as a field
        _ => Some(format!("# {tag}\n")),
    }
}

/// Returns true when the file extension suggests a non-text binary format.
fn is_binary_ext(ext: &str) -> bool {
    matches!(
        ext,
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "ico" | "bin" | "wasm"
    )
}

/// Build the final file content with the managed-by header prepended (or injected for JSON).
fn build_content(raw: &[u8], skill_name: &str, ext: &str) -> anyhow::Result<Vec<u8>> {
    if is_binary_ext(ext) {
        return Ok(raw.to_vec());
    }

    if ext == "json" {
        // Parse and inject "_managed_by" field at the top level.
        let mut val: serde_json::Value =
            serde_json::from_slice(raw).with_context(|| "failed to parse JSON skill file")?;
        if let Some(obj) = val.as_object_mut() {
            obj.insert(
                "_managed_by".to_string(),
                Value::String(format!("ff-rdp v{VERSION} skill={skill_name}")),
            );
        }
        let out = serde_json::to_string_pretty(&val)
            .context("failed to serialize JSON skill file")?
            + "\n";
        return Ok(out.into_bytes());
    }

    let header = managed_by_header(skill_name, ext).unwrap_or_default();
    let mut out = header.into_bytes();
    out.extend_from_slice(raw);
    Ok(out)
}

/// Check whether existing file content contains any ff-rdp managed-by marker
/// (any version, any skill).
fn has_any_managed_by(content: &[u8]) -> bool {
    if let Ok(s) = std::str::from_utf8(content) {
        s.contains("managed-by: ff-rdp") || s.contains("\"_managed_by\"")
    } else {
        false
    }
}

// ---------------------------------------------------------------------------
// Path resolution
// ---------------------------------------------------------------------------

/// Resolve the installation root directory for the given scope.
fn resolve_install_root(scope: SkillScope, force: bool) -> Result<PathBuf, AppError> {
    match scope {
        SkillScope::User => {
            let home = dirs::home_dir()
                .ok_or_else(|| AppError::User("could not determine home directory".to_string()))?;
            Ok(home.join(".claude").join("skills"))
        }
        SkillScope::Project => {
            let git_root = find_git_root(std::env::current_dir().map_err(|e| {
                AppError::Internal(anyhow::anyhow!("failed to get cwd: {e}"))
            })?)
            .ok_or_else(|| {
                let msg = "not in a git repository — cannot use --project scope.\n\
                           hint: run from inside a git repo, or use --user for a user-level install.\n\
                           If you really want to install here, pass --force.";
                AppError::User(msg.to_string())
            });
            if git_root.is_err() && force {
                // With --force, fall back to CWD
                let cwd = std::env::current_dir()
                    .map_err(|e| AppError::Internal(anyhow::anyhow!("failed to get cwd: {e}")))?;
                return Ok(cwd.join(".claude").join("skills"));
            }
            Ok(git_root?.join(".claude").join("skills"))
        }
    }
}

/// Walk up from `start` looking for a `.git` directory or file.
fn find_git_root(start: PathBuf) -> Option<PathBuf> {
    let mut dir = start;
    loop {
        if dir.join(".git").exists() {
            return Some(dir);
        }
        if !dir.pop() {
            return None;
        }
    }
}

// ---------------------------------------------------------------------------
// Install logic
// ---------------------------------------------------------------------------

#[derive(Debug)]
enum FileAction {
    Write,
    Skip, // already up-to-date
    DryRun,
}

struct FileResult {
    path: PathBuf,
    action: FileAction,
}

/// Install one skill dir into `install_root/<skill_name>/`.
/// Returns a list of per-file results.
fn install_skill_dir(
    skill_name: &str,
    skill_dir: &Dir<'static>,
    install_root: &Path,
    dry_run: bool,
    force: bool,
    from_dir: Option<&Path>,
) -> Result<Vec<FileResult>, AppError> {
    let dest_base = install_root.join(skill_name);
    let mut results = Vec::new();

    if let Some(from) = from_dir {
        // Read from disk instead of embedded data.
        install_from_disk_dir(from, &dest_base, skill_name, dry_run, force, &mut results)
            .map_err(AppError::Internal)?;
    } else {
        install_from_embedded(
            skill_dir,
            &dest_base,
            skill_name,
            dry_run,
            force,
            &mut results,
        )
        .map_err(AppError::Internal)?;
    }

    Ok(results)
}

fn install_from_embedded(
    dir: &Dir<'static>,
    dest_base: &Path,
    skill_name: &str,
    dry_run: bool,
    force: bool,
    results: &mut Vec<FileResult>,
) -> anyhow::Result<()> {
    for entry in dir.entries() {
        match entry {
            DirEntry::File(f) => {
                let rel = f.path();
                let dest = dest_base.join(rel);
                let ext = rel.extension().and_then(|e| e.to_str()).unwrap_or("");
                let content = build_content(f.contents(), skill_name, ext)?;
                handle_file_write(&dest, &content, dry_run, force, results)?;
            }
            DirEntry::Dir(sub) => {
                install_from_embedded(sub, dest_base, skill_name, dry_run, force, results)?;
            }
        }
    }
    Ok(())
}

fn install_from_disk_dir(
    src: &Path,
    dest_base: &Path,
    skill_name: &str,
    dry_run: bool,
    force: bool,
    results: &mut Vec<FileResult>,
) -> anyhow::Result<()> {
    for entry in
        fs::read_dir(src).with_context(|| format!("failed to read --from-dir {}", src.display()))?
    {
        let entry = entry.context("failed to read dir entry")?;
        let path = entry.path();
        let meta = entry.metadata().context("failed to read metadata")?;
        if meta.is_dir() {
            let subdir_name = path.file_name().context("directory entry has no name")?;
            let sub_dest = dest_base.join(subdir_name);
            install_from_disk_dir(&path, &sub_dest, skill_name, dry_run, force, results)?;
        } else if meta.is_file() {
            let file_name = path.file_name().context("file entry has no name")?;
            let dest = dest_base.join(file_name);
            let raw =
                fs::read(&path).with_context(|| format!("failed to read {}", path.display()))?;
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            let content = build_content(&raw, skill_name, ext)?;
            handle_file_write(&dest, &content, dry_run, force, results)?;
        }
    }
    Ok(())
}

fn handle_file_write(
    dest: &Path,
    content: &[u8],
    dry_run: bool,
    force: bool,
    results: &mut Vec<FileResult>,
) -> anyhow::Result<()> {
    if dest.exists() {
        let existing = fs::read(dest)
            .with_context(|| format!("failed to read existing file {}", dest.display()))?;

        if existing == content {
            results.push(FileResult {
                path: dest.to_path_buf(),
                action: FileAction::Skip,
            });
            return Ok(());
        }

        if !has_any_managed_by(&existing) && !force {
            return Err(anyhow::anyhow!(
                "file {} exists but is not managed by ff-rdp — refusing to overwrite.\n\
                 hint: pass --force to overwrite unmanaged files, or delete the file first.",
                dest.display()
            ));
        }

        // Managed by a different version, or --force: overwrite.
    }

    if dry_run {
        results.push(FileResult {
            path: dest.to_path_buf(),
            action: FileAction::DryRun,
        });
        return Ok(());
    }

    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create directory {}", parent.display()))?;
    }
    fs::write(dest, content).with_context(|| format!("failed to write {}", dest.display()))?;

    results.push(FileResult {
        path: dest.to_path_buf(),
        action: FileAction::Write,
    });
    Ok(())
}

// ---------------------------------------------------------------------------
// Collect all files from a skill (embedded or disk) for uninstall verification
// ---------------------------------------------------------------------------

fn collect_skill_file_names(skill_dir: &Dir<'static>) -> Vec<PathBuf> {
    let mut names = Vec::new();
    collect_embedded_names(skill_dir, &mut names);
    names
}

fn collect_embedded_names(dir: &Dir<'static>, out: &mut Vec<PathBuf>) {
    for entry in dir.entries() {
        match entry {
            DirEntry::File(f) => out.push(f.path().to_path_buf()),
            DirEntry::Dir(sub) => collect_embedded_names(sub, out),
        }
    }
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

pub fn run(cli: &Cli, args: &InstallSkillArgs) -> Result<(), AppError> {
    let pipeline = OutputPipeline::from_cli(cli)?;

    // --list
    if args.list {
        return run_list(cli, args, &pipeline);
    }

    // --uninstall
    if let Some(ref name) = args.uninstall {
        return run_uninstall(cli, args, name, &pipeline);
    }

    // install (or --dry-run)
    run_install(cli, args, &pipeline)
}

// ---------------------------------------------------------------------------
// --list
// ---------------------------------------------------------------------------

fn run_list(
    _cli: &Cli,
    args: &InstallSkillArgs,
    pipeline: &OutputPipeline,
) -> Result<(), AppError> {
    let scope = args.effective_scope();
    let install_root = resolve_install_root(scope, args.force)?;

    let items: Vec<Value> = registry()
        .iter()
        .map(|skill| {
            let install_path = install_root.join(skill.name);
            let installed = install_path.exists();
            json!({
                "name": skill.name,
                "version": VERSION,
                "installed": installed,
                "installed_path": if installed { install_path.to_string_lossy().into_owned() } else { String::new() },
            })
        })
        .collect();

    let total = items.len();
    let results = Value::Array(items);
    let meta = json!({ "scope": scope.as_str() });
    let envelope = output::envelope(&results, total, &meta);
    pipeline.finalize(&envelope).map_err(AppError::Internal)
}

// ---------------------------------------------------------------------------
// --uninstall
// ---------------------------------------------------------------------------

fn run_uninstall(
    _cli: &Cli,
    args: &InstallSkillArgs,
    name: &str,
    pipeline: &OutputPipeline,
) -> Result<(), AppError> {
    let skill_def = registry().iter().find(|s| s.name == name).ok_or_else(|| {
        AppError::User(format!(
            "unknown skill '{name}'; run --list to see available skills"
        ))
    })?;

    let scope = args.effective_scope();
    let install_root = resolve_install_root(scope, args.force)?;
    let install_path = install_root.join(name);

    if !install_path.exists() {
        let meta = json!({ "scope": scope.as_str() });
        let result = json!({ "uninstalled": false, "reason": "not installed", "path": install_path.to_string_lossy() });
        let envelope = output::envelope(&result, 1, &meta);
        return pipeline.finalize(&envelope).map_err(AppError::Internal);
    }

    // Check for user-modified files unless --force.
    if !args.force {
        let known_names = collect_skill_file_names(skill_def.dir);
        // Walk the installed directory and check for unknown files.
        check_uninstall_safety(&install_path, &known_names)?;
    }

    fs::remove_dir_all(&install_path).map_err(|e| {
        AppError::Internal(anyhow::anyhow!(
            "failed to remove {}: {e}",
            install_path.display()
        ))
    })?;

    let meta = json!({ "scope": scope.as_str() });
    let result = json!({ "uninstalled": true, "path": install_path.to_string_lossy() });
    let envelope = output::envelope(&result, 1, &meta);
    pipeline.finalize(&envelope).map_err(AppError::Internal)
}

fn check_uninstall_safety(install_path: &Path, known_names: &[PathBuf]) -> Result<(), AppError> {
    let unknown =
        find_unknown_files(install_path, install_path, known_names).map_err(AppError::Internal)?;
    if !unknown.is_empty() {
        let list = unknown
            .iter()
            .map(|p| format!("  {}", p.display()))
            .collect::<Vec<_>>()
            .join("\n");
        return Err(AppError::User(format!(
            "refusing to uninstall: the following files are not part of the bundled skill \
             (possibly user-modified):\n{list}\n\
             hint: pass --force to remove them anyway."
        )));
    }
    Ok(())
}

fn find_unknown_files(base: &Path, dir: &Path, known: &[PathBuf]) -> anyhow::Result<Vec<PathBuf>> {
    let mut unknown = Vec::new();
    for entry in
        fs::read_dir(dir).with_context(|| format!("failed to read dir {}", dir.display()))?
    {
        let entry = entry.context("failed to read dir entry")?;
        let path = entry.path();
        let meta = entry.metadata().context("failed to read metadata")?;
        if meta.is_dir() {
            unknown.extend(find_unknown_files(base, &path, known)?);
        } else {
            // Relative path from base.
            let rel = path.strip_prefix(base).unwrap_or(&path);
            if !known.iter().any(|k| k == rel) {
                unknown.push(rel.to_path_buf());
            }
        }
    }
    Ok(unknown)
}

// ---------------------------------------------------------------------------
// install / dry-run
// ---------------------------------------------------------------------------

fn run_install(
    _cli: &Cli,
    args: &InstallSkillArgs,
    pipeline: &OutputPipeline,
) -> Result<(), AppError> {
    let scope = args.effective_scope();
    let install_root = resolve_install_root(scope, args.force)?;
    let dry_run = args.dry_run;
    let force = args.force;

    // Determine which skills to install.
    let skills_to_install: Vec<&SkillDef> = if let Some(ref name) = args.skill_name {
        let found = registry()
            .iter()
            .find(|s| s.name == name.as_str())
            .ok_or_else(|| {
                AppError::User(format!(
                    "unknown skill '{name}'; run --list to see available skills"
                ))
            })?;
        vec![found]
    } else {
        registry().iter().collect()
    };

    let mut all_results: Vec<Value> = Vec::new();

    for skill in &skills_to_install {
        let file_results = install_skill_dir(
            skill.name,
            skill.dir,
            &install_root,
            dry_run,
            force,
            args.from_dir.as_deref(),
        )?;

        for fr in file_results {
            let action_str = match fr.action {
                FileAction::Write => "written",
                FileAction::Skip => "skipped",
                FileAction::DryRun => "would-write",
            };
            all_results.push(json!({
                "skill": skill.name,
                "path": fr.path.to_string_lossy(),
                "action": action_str,
            }));
        }
    }

    let total = all_results.len();
    let results = Value::Array(all_results);
    let meta = json!({
        "scope": scope.as_str(),
        "dry_run": dry_run,
    });
    let envelope = output::envelope(&results, total, &meta);
    pipeline.finalize(&envelope).map_err(AppError::Internal)
}
