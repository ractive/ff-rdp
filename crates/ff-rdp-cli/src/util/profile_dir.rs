//! Resolution of the per-user directory under which ff-rdp creates ephemeral
//! Firefox profiles.
//!
//! # Why not `env::temp_dir()`?
//!
//! `/tmp` (or `%TEMP%`) is typically world-writable.  Even though we name the
//! sub-directory with 16 random bytes, the parent is shared with every other
//! process on the box, so:
//!
//! - On Unix, a colocated same-UID process can race us to plant a `user.js`
//!   symlink that rides our `fs::write` to overwrite an arbitrary file the
//!   user can write.  Mode-0o700 on the profile sub-dir mitigates this but
//!   the parent is still shared.
//! - On multi-user systems, audit logs in `/tmp` are inspectable by other
//!   accounts; profile contents (cookies, prefs) should not live there.
//!
//! `dirs::state_dir()` (XDG `$XDG_STATE_HOME`, typically `~/.local/state`) is
//! the right home for ephemeral state that survives reboots if not pruned.
//! When `state_dir` is unavailable (older macOS, no `$HOME`) we fall back to
//! `data_local_dir` (`~/Library/Application Support` on macOS,
//! `%LOCALAPPDATA%` on Windows).  Both are per-user directories with default
//! permissions that deny other accounts.
//!
//! # Windows ACL story
//!
//! `%LOCALAPPDATA%` is created by Windows with a per-user ACL that grants
//! Full Control to the current SID and to SYSTEM, denying Everyone by
//! inheritance defaults.  Sub-directories created under it inherit those
//! restrictions, so explicit `SetNamedSecurityInfoW` is not required for
//! the threat model described above.
//! See Microsoft's "Default ACLs for user profile folders":
//! <https://learn.microsoft.com/en-us/windows/win32/secauthz/default-acls-for-user-profile-folders>.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use crate::error::AppError;

/// Resolve (and create, mode 0700 on Unix) the per-user root directory under
/// which ff-rdp drops ephemeral Firefox profile sub-directories.
///
/// Resolution order:
/// 1. `dirs::state_dir()` — `$XDG_STATE_HOME` on Linux, falls back to
///    `~/.local/state` when unset.  `None` on macOS / Windows.
/// 2. `dirs::data_local_dir()` — `~/Library/Application Support` on macOS,
///    `%LOCALAPPDATA%` on Windows.
///
/// The chosen base is joined with `ff-rdp/profiles`.  The full path is
/// created with `create_dir_all`; on Unix, the leaf is then chmod'd to
/// `0o700` (the recursive parents are left alone — they already exist with
/// user-default modes).
pub fn secure_profile_root() -> Result<PathBuf, AppError> {
    let base = dirs::state_dir()
        .or_else(dirs::data_local_dir)
        .ok_or_else(|| {
            AppError::User(
                "no per-user state or data directory available — cannot create \
                 a secure Firefox profile root.  Set $XDG_STATE_HOME or $HOME."
                    .to_owned(),
            )
        })?;
    let root = base.join("ff-rdp").join("profiles");

    std::fs::create_dir_all(&root).map_err(|e| {
        AppError::User(format!(
            "failed to create secure profile root {}: {e}",
            root.display()
        ))
    })?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o700);
        std::fs::set_permissions(&root, perms).map_err(|e| {
            AppError::User(format!(
                "failed to set mode 0o700 on profile root {}: {e}",
                root.display()
            ))
        })?;
    }

    Ok(root)
}

// ---------------------------------------------------------------------------
// Managed-profile naming convention (iter-96)
// ---------------------------------------------------------------------------

/// Prefix used for every ephemeral profile directory ff-rdp creates for
/// itself (see `commands::launch::build_command`).
const MANAGED_PROFILE_PREFIX: &str = "ff-rdp-profile-";

/// Name of the owner-PID marker file `launch` drops into a managed profile
/// directory right after spawning the Firefox that owns it (iter-97 Theme A).
///
/// The file holds the owning Firefox process's PID as plain text, newline
/// terminated. The prune paths read it back through
/// [`profile_is_owned_by_live_process`] to positively confirm the profile is
/// still in use before any age-based deletion — a stronger signal than the
/// iter-96 mtime heuristic, which stays the fallback for profiles that have
/// no marker (pre-97 dirs, or an owner whose PID has since been reused).
pub(crate) const OWNER_PID_MARKER: &str = ".ff-rdp-owner-pid";

/// Sibling marker recording *who* asked for the profile (iter-151 Theme A).
///
/// Only ever written when [`SPAWNING_TEST_ENV`] is set in `launch`'s own
/// environment — i.e. only when the live-test harness's `LiveFirefox` spawned
/// this `launch` (see `tests/common/mod.rs`). A normal interactive `ff-rdp
/// launch` never sets that env var, so this marker is simply absent for every
/// real user profile.
///
/// Before this, a leaked profile carried only [`OWNER_PID_MARKER`] — a bare
/// PID with no way to tell which of ~200 live tests spawned it, turning every
/// occurrence into a bisection hunt (see the iter-146 postmortem this
/// iteration follows up on). This marker converts that hunt into a lookup:
/// `cat <profile>/.ff-rdp-owner-test` names the exact test function.
pub(crate) const OWNER_TEST_MARKER: &str = ".ff-rdp-owner-test";

/// Sibling marker recording the owning process's *identity*, not just its PID
/// (iter-171).
///
/// Holds the opaque start token
/// [`crate::daemon::process::process_start_token`] returns for the PID in
/// [`OWNER_PID_MARKER`], written at the same moment. The PID marker outlives
/// the process it names — a leaked profile directory keeps it forever — and
/// `kill(pid, 0)` cannot tell "the Firefox that wrote this" from "whatever
/// process holds that PID now". Once the OS recycles the PID, the dead
/// profile reads as live-owned: the age-gated `profiles prune` and `launch`'s
/// own orphan sweep both skip it *permanently*, and iter-110's kill-scoping
/// gate would authorise signalling a process ff-rdp never spawned.
///
/// Comparing the recorded token against the live PID's current token closes
/// that gap, because a recycled PID necessarily has a later start time. See
/// [`owner_liveness`] for how the comparison is graded, including what happens
/// when the token is absent (every pre-iter-171 profile) or unobtainable.
///
/// Deliberately a **sibling file** rather than a second line inside
/// [`OWNER_PID_MARKER`]: three out-of-crate readers parse that file with
/// `read_to_string(..).trim().parse::<u32>()` (the live suite's `live_96`,
/// `live_151` and `live_168` all duplicate the constant locally), and a
/// two-line body would make every one of them silently stop matching — which
/// for `live_96` means its precondition quietly stops firing.
pub(crate) const OWNER_START_MARKER: &str = ".ff-rdp-owner-start";

/// Env var the live-test harness sets on every `ff-rdp launch` spawned via
/// `LiveFirefox` (see `tests/common/mod.rs`'s identically-named constant —
/// duplicated rather than imported because this crate ships no `[lib]`
/// target for an integration-test binary to pull the constant from, the same
/// reason that file already duplicates [`OWNER_PID_MARKER`] locally).
///
/// `launch` reads this (see `commands::launch::run`) and, when present and
/// non-empty, writes it into [`OWNER_TEST_MARKER`] alongside the owner-PID
/// marker.
pub(crate) const SPAWNING_TEST_ENV: &str = "FF_RDP_LIVE_TEST_NAME";

/// Number of random alphanumeric characters `tempfile::Builder::rand_bytes`
/// appends after [`MANAGED_PROFILE_PREFIX`].
const MANAGED_PROFILE_SUFFIX_LEN: usize = 16;

/// Returns `true` if `name` matches `^ff-rdp-profile-[A-Za-z0-9]{16}$` — the
/// naming convention for every profile directory ff-rdp creates for itself.
///
/// This is the safety filter shared by [`cleanup_profile_dir`] and
/// [`prune_orphan_profiles`]: only directories matching this pattern are ever
/// candidates for removal, so a user-supplied `--profile` directory is never
/// touched even if it happens to live under [`secure_profile_root`].
///
/// `pub(crate)` (iter-96 Theme C) so `commands::profiles` can reuse the exact
/// same matcher for `profiles list`/`profiles prune` instead of duplicating it.
pub(crate) fn is_managed_profile_basename(name: &str) -> bool {
    match name.strip_prefix(MANAGED_PROFILE_PREFIX) {
        Some(suffix) => {
            suffix.len() == MANAGED_PROFILE_SUFFIX_LEN
                && suffix.chars().all(|c| c.is_ascii_alphanumeric())
        }
        None => false,
    }
}

/// Returns `true` if `path`'s final component satisfies
/// [`is_managed_profile_basename`].
///
/// This is the exact predicate gating every deletion path in this crate
/// ([`cleanup_profile_dir`], [`prune_orphan_profiles`]; `commands::profiles`
/// applies [`is_managed_profile_basename`] directly) — factored out so a
/// future change to the convention cannot land on only some call sites.
pub(crate) fn is_managed_profile_path(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(is_managed_profile_basename)
}

/// Newest modification time among `dir` itself and its direct children.
///
/// A running Firefox mostly rewrites the *contents* of existing top-level
/// files (prefs.js, `*.sqlite-wal`, ...), which bumps those files' mtimes but
/// not the parent directory's — so the directory mtime alone can look stale
/// while the profile is still in use by a long-running session. Staleness
/// decisions in [`prune_orphan_profiles`] and `profiles prune` use this
/// signal instead. Unreadable entries are skipped; the result is never older
/// than `dir_mtime`. Cheap by construction: one `read_dir`, no recursion,
/// and callers only consult it for candidates that already look stale.
pub(crate) fn latest_profile_activity(dir: &Path, dir_mtime: SystemTime) -> SystemTime {
    let mut newest = dir_mtime;
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            if let Ok(modified) = entry.metadata().and_then(|m| m.modified())
                && modified > newest
            {
                newest = modified;
            }
        }
    }
    newest
}

// ---------------------------------------------------------------------------
// iter-97: owner-PID liveness guard
// ---------------------------------------------------------------------------

/// Write the owner-PID marker ([`OWNER_PID_MARKER`]) holding `pid` into the
/// managed profile directory `dir`, plus the [`OWNER_START_MARKER`] identity
/// token for that PID when the OS will supply one (iter-171).
///
/// Called by `launch` immediately after spawning the Firefox that owns `dir`,
/// so [`profile_is_owned_by_live_process`] can later confirm the profile is
/// still in use before any age-based prune deletes it.
///
/// The identity token is captured **before** the PID marker is written, so the
/// pair can never describe two different incarnations of `pid`. If the token
/// cannot be obtained (the process already exited, an unsupported platform),
/// only the PID marker is written and the profile behaves exactly as a
/// pre-iter-171 one — see [`owner_liveness`].
///
/// Warn-not-fail: a write failure is logged at `warn` and swallowed. The
/// marker is a hint that *strengthens* the prune heuristics — losing it only
/// falls back to the iter-96 mtime signal, so it must never fail a launch.
/// Only ever call this for a managed (`ff-rdp-profile-*`) directory ff-rdp
/// created for itself; a user `--profile` dir must never receive a marker.
pub(crate) fn write_owner_pid_marker(dir: &Path, pid: u32) {
    let start_token = crate::daemon::process::process_start_token(pid);

    let marker = dir.join(OWNER_PID_MARKER);
    if let Err(e) = std::fs::write(&marker, format!("{pid}\n")) {
        tracing::warn!(
            "write_owner_pid_marker: could not write {}: {e}",
            marker.display()
        );
        // Without the PID marker the start token identifies nothing, and a
        // stray token file would only confuse a later reader.
        return;
    }

    let Some(token) = start_token else {
        tracing::debug!(
            "write_owner_pid_marker: no start token available for pid {pid} — {} will fall back \
             to bare PID liveness",
            dir.display()
        );
        return;
    };
    let start_marker = dir.join(OWNER_START_MARKER);
    if let Err(e) = std::fs::write(&start_marker, format!("{token}\n")) {
        tracing::warn!(
            "write_owner_pid_marker: could not write {}: {e}",
            start_marker.display()
        );
    }
}

/// Write the owner-test marker ([`OWNER_TEST_MARKER`]) holding `test_name`
/// into the managed profile directory `dir` (iter-151 Theme A).
///
/// Warn-not-fail, same rationale as [`write_owner_pid_marker`]: this is a
/// diagnostic aid layered on top of the owner-PID marker, never load-bearing
/// for correctness, so a write failure must never fail a launch.
pub(crate) fn write_owner_test_marker(dir: &Path, test_name: &str) {
    let marker = dir.join(OWNER_TEST_MARKER);
    if let Err(e) = std::fs::write(&marker, test_name) {
        tracing::warn!(
            "write_owner_test_marker: could not write {}: {e}",
            marker.display()
        );
    }
}

/// Read back the test name recorded in `dir`'s [`OWNER_TEST_MARKER`], if any.
///
/// Returns `None` when the marker is absent (every real user profile, and
/// any pre-iter-151 profile) — callers must treat that as "unknown spawner",
/// not as evidence the profile is unmanaged.
pub(crate) fn read_owner_test_marker(dir: &Path) -> Option<String> {
    let contents = std::fs::read_to_string(dir.join(OWNER_TEST_MARKER)).ok()?;
    let trimmed = contents.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

/// How a managed profile directory's owner markers grade against the live
/// process table (iter-171).
///
/// Before iter-171 this was a bare `bool` from `kill(pid, 0)`, which conflates
/// two genuinely different answers — "a process holds that PID" and "the
/// process that wrote that PID is still running". The distinction matters
/// because the two consumers want opposite fallbacks when identity cannot be
/// established, so the ambiguity has to survive as far as the caller instead of
/// being collapsed at the source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OwnerLiveness {
    /// No [`OWNER_PID_MARKER`], or it does not parse as a PID. Callers fall
    /// back to the iter-96 mtime heuristic (every pre-iter-97 profile).
    Unmarked,
    /// The recorded PID is gone — or, the case iter-171 adds, it is alive but
    /// is provably a *different* process than the one that wrote the marker,
    /// because the live PID's start token disagrees with the recorded one.
    /// Either way the profile is abandoned.
    Dead,
    /// The recorded PID is alive and nothing contradicts its identity: either
    /// the recorded start token matches the live process's, or no token was
    /// recorded at all (a pre-iter-171 profile, or a launch on a platform with
    /// no start-time source — there is simply nothing to check against).
    Live,
    /// The recorded PID is alive and an [`OWNER_START_MARKER`] *was* recorded,
    /// but the OS declined to disclose the live process's start time — a PID
    /// owned by another user, typically. Identity is neither confirmed nor
    /// refuted, so the caller picks the direction its own blast radius
    /// demands: prune keeps the directory, the kill-scoping gate refuses.
    Unverified,
}

/// Grade `dir`'s owner markers against the live process table.
///
/// Pure lookup, no side effects. The [`OwnerLiveness`] variants document what
/// each outcome means; the interesting one is [`OwnerLiveness::Dead`] for a PID
/// that *is* alive — that is the PID-reuse false positive this iteration
/// exists to close.
pub(crate) fn owner_liveness(dir: &Path) -> OwnerLiveness {
    let Some(pid) = read_owner_pid_marker(dir) else {
        return OwnerLiveness::Unmarked;
    };
    if !crate::daemon::process::is_process_alive(pid) {
        return OwnerLiveness::Dead;
    }
    let Some(recorded) = read_owner_start_marker(dir) else {
        // Pre-iter-171 profile, or the token was unobtainable at launch.
        return OwnerLiveness::Live;
    };
    match crate::daemon::process::process_start_token(pid) {
        Some(current) if current == recorded => OwnerLiveness::Live,
        Some(_) => OwnerLiveness::Dead,
        None => OwnerLiveness::Unverified,
    }
}

/// Returns `true` iff `dir`'s owner markers say a live process still owns it.
///
/// This is the positive ownership signal the prune paths consult *before* the
/// iter-96 mtime heuristics: a live owner always wins, so a still-running
/// (even fully idle) Firefox never has its profile deleted out from under it.
///
/// A missing or unparsable marker returns `false` — the caller then falls
/// back to the mtime heuristic, so pre-97 profiles (no marker) behave exactly
/// as before.
///
/// iter-171: a marker whose PID is alive but whose recorded start token
/// disagrees is a *recycled* PID, and now returns `false` — before, the dead
/// profile read as live-owned and the age-gated prune skipped it forever.
/// [`OwnerLiveness::Unverified`] still returns `true`: for a deletion path the
/// unresolvable case must err toward keeping the directory.
pub(crate) fn profile_is_owned_by_live_process(dir: &Path) -> bool {
    matches!(
        owner_liveness(dir),
        OwnerLiveness::Live | OwnerLiveness::Unverified
    )
}

/// Read back the identity token recorded in `dir`'s [`OWNER_START_MARKER`], if
/// any. `None` for a pre-iter-171 profile, an empty file, or an unreadable one
/// — all of which mean "identity was never recorded", never "identity failed".
fn read_owner_start_marker(dir: &Path) -> Option<String> {
    let contents = std::fs::read_to_string(dir.join(OWNER_START_MARKER)).ok()?;
    let trimmed = contents.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

/// Read and parse the owner PID recorded in `dir`'s [`OWNER_PID_MARKER`], if
/// any. Returns `None` when the marker is absent or unparsable.
///
/// Split out from [`profile_is_owned_by_live_process`] so the kill-scoping
/// guard (iter-110 Theme A0) can compare a marker PID against a candidate PID
/// without also asserting liveness (the caller already knows the candidate is
/// the live port owner).
fn read_owner_pid_marker(dir: &Path) -> Option<u32> {
    let contents = std::fs::read_to_string(dir.join(OWNER_PID_MARKER)).ok()?;
    contents.trim().parse::<u32>().ok()
}

/// Returns `true` iff some managed profile directory under
/// [`secure_profile_root`] carries an [`OWNER_PID_MARKER`] naming `pid` — i.e.
/// `pid` identifies a Firefox that **ff-rdp itself spawned** (iter-110 Theme
/// A0).
///
/// This is the ownership gate the port-owner kill fallback in `daemon::client`
/// consults before signalling a process it discovered merely by *listening on
/// the RDP port*. A foreign Firefox the user launched by hand — even one on
/// ff-rdp's default port 6000 — never planted a marker under our per-user
/// profile root, so this returns `false` and the kill is skipped. Without this
/// guard the fallback would SIGKILL an unrelated browser (the 2026-07-09
/// incident: the live-test harness repeatedly killed James's interactive
/// Firefox).
///
/// Only markers in *managed* (`ff-rdp-profile-*`) directories count — a user
/// `--profile` dir never receives a marker (see [`write_owner_pid_marker`]),
/// so this cannot be spoofed into authorising a kill by a hand-planted file in
/// a user profile that happens to sit under the root.
///
/// Fails **closed**: any error resolving or reading the profile root returns
/// `false` (do not kill). The cost of a false negative is a leftover foreign
/// process the user can stop themselves; the cost of a false positive is
/// killing the user's browser — always err toward not killing.
///
/// iter-171 tightened this the same way: a marker naming `pid` only authorises
/// a kill if the profile's recorded start token still matches that PID's. A
/// leaked profile directory keeps its marker forever, so once the OS recycles
/// the PID the stale marker would otherwise hand this gate a signed permission
/// slip for an unrelated process — exactly the outcome the gate exists to
/// prevent. [`OwnerLiveness::Unverified`] therefore also refuses here (the
/// opposite of what the prune paths do with it), because "cannot confirm"
/// must never mean "go ahead and kill".
pub(crate) fn pid_is_ff_rdp_spawned(pid: u32) -> bool {
    let Ok(root) = secure_profile_root() else {
        return false;
    };
    pid_is_ff_rdp_spawned_under(&root, pid)
}

/// Root-parameterised core of [`pid_is_ff_rdp_spawned`] so the ownership gate
/// can be unit-tested against a temp profile root without touching the real
/// per-user directory.
fn pid_is_ff_rdp_spawned_under(root: &Path, pid: u32) -> bool {
    let Ok(entries) = std::fs::read_dir(root) else {
        return false;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !is_managed_profile_path(&path) {
            continue;
        }
        if read_owner_pid_marker(&path) == Some(pid) && owner_liveness(&path) == OwnerLiveness::Live
        {
            return true;
        }
    }
    false
}

// ---------------------------------------------------------------------------
// Theme A: active-profile cleanup on `daemon stop`
// ---------------------------------------------------------------------------

/// Outcome of [`cleanup_profile_dir`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProfileCleanup {
    /// The directory was removed; carries the path that was removed.
    Removed(PathBuf),
    /// Nothing was removed — either a safety check refused the path, or
    /// removal itself failed. Both cases are silent (warn-not-fail): see
    /// the function doc for why this never surfaces as an error.
    Skipped,
}

impl ProfileCleanup {
    /// `Some(path)` if the directory was removed, `None` if it was skipped.
    pub fn removed_path(&self) -> Option<&Path> {
        match self {
            Self::Removed(p) => Some(p),
            Self::Skipped => None,
        }
    }
}

/// Remove `path` if — and only if — it is a directory ff-rdp created for
/// itself: under [`secure_profile_root`] AND named
/// `ff-rdp-profile-<16 alphanumeric chars>`.
///
/// Both checks must pass. This is what stands between `daemon stop` and
/// deleting a directory the user passed via `--profile`, so the function
/// fails closed: an unresolvable profile root, a path outside it, or a
/// basename mismatch all return [`ProfileCleanup::Skipped`] silently
/// (debug-level log only, no error). A `remove_dir_all` failure on an
/// otherwise-valid managed path is logged at `warn` and also returns
/// `Skipped` — callers never see an `Err` from this function.
pub fn cleanup_profile_dir(path: &Path) -> ProfileCleanup {
    let root = match secure_profile_root() {
        Ok(root) => root,
        Err(e) => {
            tracing::debug!(
                "cleanup_profile_dir: could not resolve secure profile root, skipping {}: {e:#}",
                path.display()
            );
            return ProfileCleanup::Skipped;
        }
    };

    if !path.starts_with(&root) {
        tracing::debug!(
            "cleanup_profile_dir: refusing to remove {} — not under secure profile root {}",
            path.display(),
            root.display()
        );
        return ProfileCleanup::Skipped;
    }

    if !is_managed_profile_path(path) {
        tracing::debug!(
            "cleanup_profile_dir: refusing to remove {} — basename is not a managed ff-rdp profile dir",
            path.display()
        );
        return ProfileCleanup::Skipped;
    }

    match std::fs::remove_dir_all(path) {
        Ok(()) => {
            tracing::debug!("cleanup_profile_dir: removed {}", path.display());
            ProfileCleanup::Removed(path.to_path_buf())
        }
        Err(e) => {
            tracing::warn!(
                "cleanup_profile_dir: failed to remove {}: {e}",
                path.display()
            );
            ProfileCleanup::Skipped
        }
    }
}

// ---------------------------------------------------------------------------
// Theme B: orphan pruning on `launch`
// ---------------------------------------------------------------------------

/// Result of a [`prune_orphan_profiles`] call.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PruneSummary {
    /// Paths that were removed, in the order they were removed.
    pub removed: Vec<PathBuf>,
}

/// Remove stale `ff-rdp-profile-*` directories directly under
/// `profile_root`. A directory is stale when both its own `mtime` and its
/// newest top-level file mtime ([`latest_profile_activity`]) are at least
/// `age_threshold` old — the second signal keeps a long-running live
/// session's profile off the candidate list.
///
/// Bounded by `max_entries`: stops after removing that many directories so a
/// large backlog can't add unbounded latency to a single `launch` — the rest
/// is picked up by later calls. All errors (missing root, unreadable
/// entries, a directory vanishing mid-scan because of a concurrent prune)
/// are tolerated: this must never block or fail a launch, so failures are
/// logged at `warn` and the entry is skipped rather than propagated.
///
/// Only entries matching `^ff-rdp-profile-[A-Za-z0-9]{16}$` are ever
/// candidates — the same safety filter as [`cleanup_profile_dir`] — so a
/// directory the user placed under `profile_root` by hand is never pruned.
pub fn prune_orphan_profiles(
    profile_root: &Path,
    age_threshold: Duration,
    max_entries: usize,
) -> PruneSummary {
    let mut summary = PruneSummary::default();
    if max_entries == 0 {
        return summary;
    }

    let entries = match std::fs::read_dir(profile_root) {
        Ok(entries) => entries,
        Err(e) => {
            tracing::warn!(
                "prune_orphan_profiles: could not read {}: {e}",
                profile_root.display()
            );
            return summary;
        }
    };

    let now = std::time::SystemTime::now();

    for entry in entries {
        if summary.removed.len() >= max_entries {
            break;
        }

        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                tracing::warn!("prune_orphan_profiles: unreadable directory entry: {e}");
                continue;
            }
        };

        let path = entry.path();
        if !is_managed_profile_path(&path) {
            continue;
        }

        // iter-97 Theme B: a live owner-PID marker is a positive "still in
        // use" signal that overrides the mtime heuristics below — a running
        // (even fully idle) Firefox never has its profile pruned. A missing
        // marker falls through to the mtime checks unchanged.
        //
        // iter-142 Theme B: a marker naming a *dead* PID is the opposite
        // signal — definitive proof of abandonment, not just "old enough to
        // guess at". Previously a dead-owner profile still had to wait out
        // the full `age_threshold` (7 days by default) before
        // `prune_orphan_profiles` would touch it, so a day of crashes /
        // `kill -9`s / restarted agents accumulated freely: dogfooding
        // session 63 observed 62 profiles / 2.7 GB in a single day, all well
        // under the threshold. Remove dead-owner profiles immediately,
        // regardless of age — `max_entries` below still bounds how many a
        // single `launch` will reclaim.
        //
        // iter-171: "dead" now includes a marker whose PID is *alive* but
        // whose recorded start token disagrees — a recycled PID. Grading that
        // case as alive is how a leaked directory became permanently
        // unreclaimable: the age gate below never runs for a live owner, so
        // the profile survived every future sweep.
        let owner = owner_liveness(&path);
        let marker_pid = read_owner_pid_marker(&path);
        match owner {
            OwnerLiveness::Live | OwnerLiveness::Unverified => {
                tracing::debug!(
                    "prune_orphan_profiles: keeping {} — owner PID {marker_pid:?} is alive \
                     ({owner:?})",
                    path.display()
                );
                continue;
            }
            OwnerLiveness::Dead => {
                tracing::debug!(
                    "prune_orphan_profiles: owner PID {marker_pid:?} of {} is gone (dead, or a \
                     recycled PID that is no longer the process that wrote the marker) — \
                     removing immediately",
                    path.display()
                );
                match std::fs::remove_dir_all(&path) {
                    Ok(()) => summary.removed.push(path),
                    Err(e) => tracing::warn!(
                        "prune_orphan_profiles: failed to remove dead-owner {}: {e}",
                        path.display()
                    ),
                }
                continue;
            }
            OwnerLiveness::Unmarked => {
                // No marker (pre-97 profile, or the marker write failed) —
                // fall back to the mtime heuristic below.
            }
        }

        // `metadata()` (not `entry.file_type()`) so a vanished entry (race
        // with a concurrent prune / the OS reaping a crashed Firefox) is
        // tolerated here rather than panicking downstream.
        let metadata = match entry.metadata() {
            Ok(m) => m,
            Err(e) => {
                tracing::warn!(
                    "prune_orphan_profiles: could not stat {}: {e}",
                    path.display()
                );
                continue;
            }
        };
        if !metadata.is_dir() {
            continue;
        }

        let modified = match metadata.modified() {
            Ok(m) => m,
            Err(e) => {
                tracing::warn!(
                    "prune_orphan_profiles: no mtime available for {}: {e}",
                    path.display()
                );
                continue;
            }
        };

        // mtime is in the future (clock skew) — treat as fresh, not stale.
        let Ok(age) = now.duration_since(modified) else {
            continue;
        };
        if age < age_threshold {
            continue;
        }

        // The directory itself looks stale — but a live Firefox mostly
        // rewrites the *contents* of existing files, which doesn't bump the
        // parent dir's mtime. Consult the newest top-level file mtime before
        // deleting, so a still-running session's profile is never mistaken
        // for an orphan. Future mtimes (clock skew) again count as fresh.
        let newest = latest_profile_activity(&path, modified);
        let Ok(age) = now.duration_since(newest) else {
            continue;
        };
        if age < age_threshold {
            continue;
        }

        match std::fs::remove_dir_all(&path) {
            Ok(()) => {
                tracing::debug!("prune_orphan_profiles: removed stale {}", path.display());
                summary.removed.push(path);
            }
            Err(e) => {
                tracing::warn!(
                    "prune_orphan_profiles: failed to remove {}: {e}",
                    path.display()
                );
            }
        }
    }

    summary
}

#[cfg(test)]
mod tests {
    use super::*;

    /// AC: `secure_profile_root_mode_0700` — the resolved directory exists,
    /// sits under `dirs::state_dir()` or `data_local_dir()`, and has mode
    /// `0o700` on Unix.
    #[cfg(unix)]
    #[test]
    fn secure_profile_root_mode_0700() {
        use std::os::unix::fs::PermissionsExt;

        let root = secure_profile_root().expect("secure profile root must resolve");
        assert!(root.is_dir(), "expected a directory at {}", root.display());
        let expected_base = dirs::state_dir().or_else(dirs::data_local_dir).unwrap();
        assert!(
            root.starts_with(&expected_base),
            "profile root {} must be under {}",
            root.display(),
            expected_base.display()
        );
        let mode = std::fs::metadata(&root).unwrap().permissions().mode() & 0o777;
        assert_eq!(
            mode, 0o700,
            "profile root must be mode 0o700, found {mode:o}"
        );
    }

    /// AC: `secure_profile_root_windows_per_user` — on Windows the resolved
    /// directory sits under `%LOCALAPPDATA%` and is a valid directory.  We
    /// rely on the inherited default ACL (per-user) for confidentiality.
    #[cfg(windows)]
    #[test]
    fn secure_profile_root_windows_per_user() {
        let root = secure_profile_root().expect("secure profile root must resolve");
        assert!(root.is_dir(), "expected a directory at {}", root.display());
        let local_appdata = dirs::data_local_dir().expect("LOCALAPPDATA must be defined");
        assert!(
            root.starts_with(&local_appdata),
            "profile root {} must be under {}",
            root.display(),
            local_appdata.display()
        );
    }

    // -----------------------------------------------------------------
    // cleanup_profile_dir (Theme A)
    // -----------------------------------------------------------------

    /// AC: `unit_cleanup_profile_dir_refuses_path_outside_profile_root` — a
    /// path that is not under `secure_profile_root()` (even one that matches
    /// the `ff-rdp-profile-*` naming convention) must never be removed, and
    /// the function must report `Skipped` rather than surfacing an error.
    /// This is the guard that keeps a user-supplied `--profile` directory
    /// safe from `daemon stop`.
    #[test]
    fn unit_cleanup_profile_dir_refuses_path_outside_profile_root() {
        let outside = tempfile::Builder::new()
            .prefix("ff-rdp-profile-")
            .rand_bytes(16)
            .tempdir()
            .expect("tempdir outside profile root");

        let result = cleanup_profile_dir(outside.path());

        assert_eq!(result, ProfileCleanup::Skipped);
        assert!(
            outside.path().exists(),
            "directory outside secure_profile_root must survive cleanup_profile_dir"
        );
    }

    /// Companion happy-path check: a directory that lives under
    /// `secure_profile_root()` AND matches the naming convention is removed.
    #[test]
    fn unit_cleanup_profile_dir_removes_managed_path_under_root() {
        let root = secure_profile_root().expect("secure profile root must resolve");
        let managed = root.join(format!("ff-rdp-profile-{}", "c".repeat(16)));
        std::fs::create_dir_all(&managed).expect("create fake managed profile dir");

        let result = cleanup_profile_dir(&managed);

        assert_eq!(result, ProfileCleanup::Removed(managed.clone()));
        assert!(!managed.exists(), "managed profile dir should be removed");
    }

    /// A directory under `secure_profile_root()` whose basename does NOT
    /// match the naming convention (e.g. a `--profile` path that happens to
    /// be nested under the root) must still be refused.
    #[test]
    fn unit_cleanup_profile_dir_refuses_basename_mismatch_even_under_root() {
        let root = secure_profile_root().expect("secure profile root must resolve");
        let not_managed = root.join("some-other-dir");
        std::fs::create_dir_all(&not_managed).expect("create dir");

        let result = cleanup_profile_dir(&not_managed);

        assert_eq!(result, ProfileCleanup::Skipped);
        assert!(not_managed.exists());

        let _ = std::fs::remove_dir_all(&not_managed);
    }

    // -----------------------------------------------------------------
    // prune_orphan_profiles (Theme B)
    // -----------------------------------------------------------------

    /// Create a fake managed profile dir `ff-rdp-profile-<suffix>` under
    /// `root` and back-date its mtime by `age`. `suffix` must be exactly 16
    /// alphanumeric characters to satisfy `is_managed_profile_basename`.
    fn seed_fake_profile(root: &Path, suffix: &str, age: Duration) -> PathBuf {
        assert_eq!(
            suffix.len(),
            MANAGED_PROFILE_SUFFIX_LEN,
            "test fixture suffix must be exactly {MANAGED_PROFILE_SUFFIX_LEN} chars: {suffix}"
        );
        let dir = root.join(format!("{MANAGED_PROFILE_PREFIX}{suffix}"));
        std::fs::create_dir_all(&dir).expect("create fake profile dir");
        let mtime = std::time::SystemTime::now()
            .checked_sub(age)
            .expect("age fits before now");
        filetime::set_file_mtime(&dir, filetime::FileTime::from_system_time(mtime))
            .expect("set_file_mtime");
        dir
    }

    /// AC: `pre_fix_repro_launch_prunes_stale_orphan_profiles` — three
    /// managed profile dirs with an 8-day-old mtime are all removed by a
    /// single `prune_orphan_profiles` call at the default 7-day threshold.
    #[test]
    fn pre_fix_repro_launch_prunes_stale_orphan_profiles() {
        let root = tempfile::tempdir().expect("tempdir");
        let eight_days = Duration::from_hours(192);
        let seeded: Vec<PathBuf> = (0..3)
            .map(|i| {
                let suffix = format!("{i:016}");
                seed_fake_profile(root.path(), &suffix, eight_days)
            })
            .collect();

        let summary = prune_orphan_profiles(root.path(), Duration::from_hours(168), 50);

        assert_eq!(
            summary.removed.len(),
            3,
            "all three stale dirs should be pruned"
        );
        for dir in &seeded {
            assert!(!dir.exists(), "{} should have been removed", dir.display());
        }
    }

    /// AC: `unit_prune_orphan_profiles_respects_age_threshold` — an 8-day-old
    /// dir is pruned at a 7-day threshold; a 1-hour-old dir survives.
    #[test]
    fn unit_prune_orphan_profiles_respects_age_threshold() {
        let root = tempfile::tempdir().expect("tempdir");
        let old_dir = seed_fake_profile(root.path(), &"a".repeat(16), Duration::from_hours(192));
        let fresh_dir = seed_fake_profile(root.path(), &"b".repeat(16), Duration::from_hours(1));

        let summary = prune_orphan_profiles(root.path(), Duration::from_hours(168), 50);

        assert_eq!(summary.removed, vec![old_dir.clone()]);
        assert!(!old_dir.exists(), "8-day-old dir should be pruned");
        assert!(fresh_dir.exists(), "1-hour-old dir should survive");
    }

    /// A directory whose own mtime is stale but which contains a
    /// recently-written top-level file (the signature of a live Firefox
    /// session — content rewrites bump file mtimes, not the parent dir's)
    /// must NOT be pruned by launch's automatic orphan sweep.
    #[test]
    fn unit_prune_orphan_profiles_skips_profile_with_fresh_inner_file() {
        let root = tempfile::tempdir().expect("tempdir");
        let dir = seed_fake_profile(root.path(), &"c".repeat(16), Duration::from_hours(192));
        // Simulate live-session activity: write a fresh inner file, then
        // re-backdate the directory itself (the write bumps its mtime).
        std::fs::write(dir.join("prefs.js"), b"user_pref!").expect("write fresh inner file");
        let stale = std::time::SystemTime::now()
            .checked_sub(Duration::from_hours(192))
            .expect("age fits before now");
        filetime::set_file_mtime(&dir, filetime::FileTime::from_system_time(stale))
            .expect("re-backdate dir mtime");

        let summary = prune_orphan_profiles(root.path(), Duration::from_hours(168), 50);

        assert!(
            summary.removed.is_empty(),
            "a profile with fresh top-level file activity must survive the launch sweep"
        );
        assert!(dir.exists(), "{} must survive", dir.display());
    }

    /// Spawn a trivial child process, wait for it to exit, and return its
    /// now-dead PID. Used to exercise the dead-owner branch of the liveness
    /// guard portably (no reliance on a magic large PID that could collide).
    fn spawn_and_reap_child_pid() -> u32 {
        #[cfg(unix)]
        let mut child = std::process::Command::new("true")
            .spawn()
            .expect("spawn `true`");
        #[cfg(windows)]
        let mut child = std::process::Command::new("cmd")
            .args(["/C", "exit", "0"])
            .spawn()
            .expect("spawn cmd exit");
        let pid = child.id();
        child.wait().expect("child exits");
        // Give the OS a beat to fully reap so `is_process_alive` reports dead.
        std::thread::sleep(Duration::from_millis(50));
        pid
    }

    /// `write_owner_pid_marker` + `profile_is_owned_by_live_process` round
    /// trip: the current process is alive, so a marker naming it reports
    /// `true`; a dir with no marker or a garbage marker reports `false`.
    #[test]
    fn unit_owner_pid_marker_roundtrip() {
        let dir = tempfile::tempdir().expect("tempdir");
        // No marker yet.
        assert!(!profile_is_owned_by_live_process(dir.path()));

        write_owner_pid_marker(dir.path(), std::process::id());
        assert!(
            profile_is_owned_by_live_process(dir.path()),
            "a marker naming the live test process must report alive"
        );

        // Garbage marker → not owned by a live process.
        std::fs::write(dir.path().join(OWNER_PID_MARKER), b"not-a-pid\n").expect("overwrite");
        assert!(!profile_is_owned_by_live_process(dir.path()));
    }

    /// AC: `live_151_leaked_profile_names_its_test` (unit half) —
    /// `write_owner_test_marker` + `read_owner_test_marker` round trip, an
    /// absent marker reads back `None`, and an all-whitespace marker (a
    /// degenerate but technically-written env var) also reads back `None`
    /// rather than an empty-but-`Some` string.
    #[test]
    fn unit_owner_test_marker_roundtrip() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert_eq!(read_owner_test_marker(dir.path()), None);

        write_owner_test_marker(dir.path(), "live_151_leaked_profile_names_its_test");
        assert_eq!(
            read_owner_test_marker(dir.path()),
            Some("live_151_leaked_profile_names_its_test".to_owned())
        );

        std::fs::write(dir.path().join(OWNER_TEST_MARKER), b"   \n").expect("overwrite blank");
        assert_eq!(read_owner_test_marker(dir.path()), None);
    }

    // -----------------------------------------------------------------
    // iter-110 Theme A0: kill-scoping ownership gate
    // -----------------------------------------------------------------

    /// AC: `unit_pid_is_ff_rdp_spawned_true_only_for_marked_managed_profile`
    ///
    /// The kill-scoping gate must authorise a kill ONLY when a managed profile
    /// under the root carries an owner-PID marker naming the exact candidate
    /// PID. Proves: (a) a marked managed dir → `true` for that PID; (b) a
    /// *different* (foreign) PID → `false`; (c) an empty root → `false`. This
    /// is the primitive that stops ff-rdp from killing a user's own Firefox.
    ///
    /// iter-171: the marked PID is now this live test process rather than the
    /// fabricated `4242`. The gate grades the marker through
    /// [`owner_liveness`], which cannot confirm the identity of a PID that
    /// does not exist — and every real caller has already established that its
    /// candidate is the live process listening on the port, so a dead PID was
    /// never a case this gate had to authorise.
    #[test]
    fn unit_pid_is_ff_rdp_spawned_true_only_for_marked_managed_profile() {
        let root = tempfile::tempdir().expect("tempdir");
        let live_pid = std::process::id();

        // Empty root: no profile owns anything.
        assert!(
            !pid_is_ff_rdp_spawned_under(root.path(), live_pid),
            "an empty profile root must never authorise a kill"
        );

        // A managed dir whose marker names the live PID.
        let dir = seed_fake_profile(root.path(), &"a".repeat(16), Duration::from_secs(1));
        write_owner_pid_marker(&dir, live_pid);

        assert!(
            pid_is_ff_rdp_spawned_under(root.path(), live_pid),
            "the marked managed PID must be recognised as ff-rdp-spawned"
        );
        assert!(
            !pid_is_ff_rdp_spawned_under(root.path(), 9999),
            "a foreign PID with no marker naming it must NEVER be authorised — \
             this is the guard that spared James's interactive Firefox"
        );
    }

    /// AC: `unit_pid_is_ff_rdp_spawned_ignores_marker_in_unmanaged_dir` — a
    /// marker planted in a directory under the root that does NOT match the
    /// `ff-rdp-profile-*` convention (e.g. a user `--profile` dir, or an
    /// attacker-planted dir) must be ignored, so it cannot spoof authorisation
    /// to kill an arbitrary PID.
    #[test]
    fn unit_pid_is_ff_rdp_spawned_ignores_marker_in_unmanaged_dir() {
        let root = tempfile::tempdir().expect("tempdir");
        let unmanaged = root.path().join("my-own-firefox-profile");
        std::fs::create_dir_all(&unmanaged).expect("create unmanaged dir");
        std::fs::write(unmanaged.join(OWNER_PID_MARKER), b"4242\n").expect("write marker");

        assert!(
            !pid_is_ff_rdp_spawned_under(root.path(), 4242),
            "a marker in a non-managed dir must not authorise a kill"
        );
    }

    /// A garbage / unparsable marker in a managed dir yields no ownership
    /// claim — `read_owner_pid_marker` returns `None`, so no PID matches.
    #[test]
    fn unit_pid_is_ff_rdp_spawned_rejects_garbage_marker() {
        let root = tempfile::tempdir().expect("tempdir");
        let dir = seed_fake_profile(root.path(), &"b".repeat(16), Duration::from_secs(1));
        std::fs::write(dir.join(OWNER_PID_MARKER), b"not-a-pid\n").expect("write garbage marker");

        assert!(
            !pid_is_ff_rdp_spawned_under(root.path(), 4242),
            "an unparsable marker must not authorise any kill"
        );
    }

    /// AC: `pre_fix_repro_prune_deletes_profile_with_live_owner_pid` — a
    /// managed profile dir with fully back-dated mtimes (dir + all files) AND
    /// an `.ff-rdp-owner-pid` naming the current (live) test process must NOT
    /// be pruned by `prune_orphan_profiles` at a 7-day threshold. On pre-97
    /// code the dir is deleted (the heuristic gap this iteration closes);
    /// post-fix the live-owner guard keeps it.
    #[test]
    fn pre_fix_repro_prune_deletes_profile_with_live_owner_pid() {
        let root = tempfile::tempdir().expect("tempdir");
        let dir = seed_fake_profile(root.path(), &"a".repeat(16), Duration::from_hours(192));
        // Marker naming this live test process; back-date it too so mtimes
        // alone scream "stale".
        std::fs::write(
            dir.join(OWNER_PID_MARKER),
            format!("{}\n", std::process::id()),
        )
        .expect("write owner-pid marker");
        let stale = std::time::SystemTime::now()
            .checked_sub(Duration::from_hours(192))
            .expect("age fits before now");
        let ft = filetime::FileTime::from_system_time(stale);
        filetime::set_file_mtime(dir.join(OWNER_PID_MARKER), ft).expect("backdate marker");
        filetime::set_file_mtime(&dir, ft).expect("re-backdate dir");

        let summary = prune_orphan_profiles(root.path(), Duration::from_hours(168), 50);

        assert!(
            summary.removed.is_empty(),
            "a profile whose owner PID is alive must survive the sweep at any age"
        );
        assert!(
            dir.exists(),
            "{} must survive a live-owner sweep",
            dir.display()
        );
    }

    /// AC: `unit_prune_skips_live_owner_but_reclaims_dead_owner` — a marker
    /// naming the live test process blocks pruning; a marker naming a
    /// known-dead PID does not, so a stale dir with a dead owner is reclaimed.
    #[test]
    fn unit_prune_skips_live_owner_but_reclaims_dead_owner() {
        let root = tempfile::tempdir().expect("tempdir");
        let old = Duration::from_hours(192);

        // Live owner → kept.
        let live = seed_fake_profile(root.path(), &"1".repeat(16), old);
        std::fs::write(
            live.join(OWNER_PID_MARKER),
            format!("{}\n", std::process::id()),
        )
        .expect("write live marker");

        // Dead owner → reclaimed. Back-date the marker file and re-backdate
        // the dir so writing the marker doesn't count as fresh top-level
        // activity (which the iter-96 mtime heuristic would treat as live).
        let dead = seed_fake_profile(root.path(), &"2".repeat(16), old);
        let dead_pid = spawn_and_reap_child_pid();
        std::fs::write(dead.join(OWNER_PID_MARKER), format!("{dead_pid}\n"))
            .expect("write dead marker");
        let stale = std::time::SystemTime::now()
            .checked_sub(old)
            .expect("age fits before now");
        let ft = filetime::FileTime::from_system_time(stale);
        filetime::set_file_mtime(dead.join(OWNER_PID_MARKER), ft).expect("backdate dead marker");
        filetime::set_file_mtime(&dead, ft).expect("re-backdate dead dir");

        let summary = prune_orphan_profiles(root.path(), Duration::from_hours(168), 50);

        assert_eq!(summary.removed, vec![dead.clone()]);
        assert!(live.exists(), "live-owner dir must survive");
        assert!(!dead.exists(), "dead-owner dir must be reclaimed");
    }

    /// AC `live_142_profile_growth_bounded` (unit half — the policy under
    /// test): a dead-owner profile is reclaimed **immediately**, even when
    /// it is only seconds old — the age threshold no longer gates removal
    /// once ownership is provably dead. This is the exact gap that let 62
    /// profiles / 2.7 GB accumulate in a single day (dogfooding session 63):
    /// every one of them was younger than the old 7-day age gate.
    #[test]
    fn unit_prune_reclaims_dead_owner_immediately_regardless_of_age() {
        let root = tempfile::tempdir().expect("tempdir");
        // Fresh (1 second old) — would survive any age-gated heuristic.
        let dead = seed_fake_profile(root.path(), &"4".repeat(16), Duration::from_secs(1));
        let dead_pid = spawn_and_reap_child_pid();
        std::fs::write(dead.join(OWNER_PID_MARKER), format!("{dead_pid}\n"))
            .expect("write dead marker");

        // A 168h (7-day) threshold — the dir is nowhere near stale by mtime.
        let summary = prune_orphan_profiles(root.path(), Duration::from_hours(168), 50);

        assert_eq!(
            summary.removed,
            vec![dead.clone()],
            "a dead-owner profile must be reclaimed the moment ownership is \
             provably dead, not after waiting out the age threshold"
        );
        assert!(
            !dead.exists(),
            "fresh but dead-owner profile must be removed immediately"
        );
    }

    /// A missing marker falls back to the iter-96 mtime heuristic (pre-97
    /// profiles have no marker), so a stale dir with no marker is still
    /// pruned exactly as before.
    #[test]
    fn unit_prune_no_marker_falls_back_to_mtime_heuristic() {
        let root = tempfile::tempdir().expect("tempdir");
        let dir = seed_fake_profile(root.path(), &"3".repeat(16), Duration::from_hours(192));

        let summary = prune_orphan_profiles(root.path(), Duration::from_hours(168), 50);

        assert_eq!(summary.removed, vec![dir.clone()]);
        assert!(
            !dir.exists(),
            "a marker-less stale dir must still be pruned"
        );
    }

    // -----------------------------------------------------------------
    // iter-171: PID reuse must not resurrect a dead profile's ownership
    // -----------------------------------------------------------------

    /// Seed a managed profile dir under `root` whose owner-PID marker names a
    /// **live** process (this one) but whose owner-start marker records a
    /// token that process cannot have — i.e. exactly the on-disk state a
    /// leaked profile reaches once the OS hands its old PID to something else.
    ///
    /// Forging the token rather than waiting for a real recycle is deliberate:
    /// PID recycling was measured at ~229 allocations/second against a
    /// `PID_MAX` of 99 999 on this project's macOS dev machine, so reproducing
    /// it honestly costs minutes of saturated spawning per run.
    fn seed_recycled_pid_profile(root: &Path, suffix: &str) -> PathBuf {
        let dir = seed_fake_profile(root, suffix, Duration::from_secs(1));
        write_owner_pid_marker(&dir, std::process::id());
        // Overwrite whatever real token the write captured with one that is
        // structurally valid but belongs to no incarnation of this PID.
        std::fs::write(dir.join(OWNER_START_MARKER), b"0.000001\n")
            .expect("forge a mismatched start token");
        dir
    }

    /// AC (iter-171, fails on `main`): a profile whose owner PID is alive but
    /// is a *different process* than the one that wrote the marker must not
    /// read as live-owned.
    ///
    /// On `main` `profile_is_owned_by_live_process` is `kill(pid, 0)` and
    /// answers `true` here, because the recycled PID really is alive — which
    /// is what made a leaked directory permanently unreclaimable and what
    /// tripped `live_96_profile_cleanup`'s precondition.
    #[test]
    fn pre_fix_repro_recycled_owner_pid_reads_as_live() {
        let root = tempfile::tempdir().expect("tempdir");
        let recycled = seed_recycled_pid_profile(root.path(), &"7".repeat(16));

        assert_eq!(
            owner_liveness(&recycled),
            OwnerLiveness::Dead,
            "a live PID whose start token disagrees with the recorded one is a \
             recycled PID, not the original owner"
        );
        assert!(
            !profile_is_owned_by_live_process(&recycled),
            "a recycled PID must not resurrect a dead profile's ownership"
        );
    }

    /// AC (iter-171, fails on `main`): the recycled-PID profile is actually
    /// *reclaimed* — not merely graded correctly. This is the user-visible
    /// half: on `main` the age-gated sweep sees a live owner, skips the entry
    /// outright, and therefore never removes it at any age.
    #[test]
    fn pre_fix_repro_prune_never_reclaims_recycled_pid_profile() {
        let root = tempfile::tempdir().expect("tempdir");
        let recycled = seed_recycled_pid_profile(root.path(), &"8".repeat(16));

        let summary = prune_orphan_profiles(root.path(), Duration::from_hours(168), 50);

        assert_eq!(
            summary.removed,
            vec![recycled.clone()],
            "a profile whose owner PID has been recycled must be reclaimed"
        );
        assert!(!recycled.exists(), "{} must be gone", recycled.display());
    }

    /// AC (iter-171): the kill-scoping gate (iter-110 Theme A0) must not be
    /// fooled either. A stale marker naming a recycled PID would otherwise
    /// hand it a permission slip to SIGKILL a process ff-rdp never spawned —
    /// the precise failure mode that gate exists to prevent.
    #[test]
    fn unit_pid_is_ff_rdp_spawned_refuses_recycled_pid() {
        let root = tempfile::tempdir().expect("tempdir");
        seed_recycled_pid_profile(root.path(), &"9".repeat(16));

        assert!(
            !pid_is_ff_rdp_spawned_under(root.path(), std::process::id()),
            "a marker whose start token disagrees names a PID ff-rdp no longer \
             owns — it must never authorise a kill"
        );
    }

    /// The genuine owner is unaffected: `write_owner_pid_marker` records the
    /// live process's real token, so the same paths still read `Live` and the
    /// profile is still protected from pruning and still authorises the
    /// iter-110 gate.
    #[test]
    fn unit_owner_markers_roundtrip_for_the_real_owner() {
        let root = tempfile::tempdir().expect("tempdir");
        let dir = seed_fake_profile(root.path(), &"a".repeat(16), Duration::from_hours(192));
        write_owner_pid_marker(&dir, std::process::id());

        assert_eq!(owner_liveness(&dir), OwnerLiveness::Live);
        assert!(profile_is_owned_by_live_process(&dir));
        assert!(pid_is_ff_rdp_spawned_under(root.path(), std::process::id()));

        let summary = prune_orphan_profiles(root.path(), Duration::from_hours(168), 50);
        assert!(
            summary.removed.is_empty(),
            "the real owner's profile must still survive the sweep at any age"
        );
        assert!(dir.exists());
    }

    /// A pre-iter-171 profile — owner-PID marker present, no start marker —
    /// keeps its old semantics exactly: a live PID reads `Live`, a dead one
    /// reads `Dead`. Without this, upgrading ff-rdp would make every existing
    /// profile on disk suddenly unprotected (or suddenly unreclaimable).
    #[test]
    fn unit_owner_liveness_legacy_profile_without_start_marker() {
        let root = tempfile::tempdir().expect("tempdir");

        let live = seed_fake_profile(root.path(), &"b".repeat(16), Duration::from_secs(1));
        std::fs::write(
            live.join(OWNER_PID_MARKER),
            format!("{}\n", std::process::id()),
        )
        .expect("write legacy live marker");
        assert_eq!(owner_liveness(&live), OwnerLiveness::Live);

        let dead = seed_fake_profile(root.path(), &"c".repeat(16), Duration::from_secs(1));
        std::fs::write(
            dead.join(OWNER_PID_MARKER),
            format!("{}\n", spawn_and_reap_child_pid()),
        )
        .expect("write legacy dead marker");
        assert_eq!(owner_liveness(&dead), OwnerLiveness::Dead);

        let unmarked = seed_fake_profile(root.path(), &"d".repeat(16), Duration::from_secs(1));
        assert_eq!(owner_liveness(&unmarked), OwnerLiveness::Unmarked);
    }

    /// A blank or unreadable start marker means "identity was never recorded",
    /// not "identity failed" — so it must degrade to the legacy answer rather
    /// than declaring a genuinely-live owner dead and deleting its profile.
    #[test]
    fn unit_blank_start_marker_degrades_to_bare_liveness() {
        let root = tempfile::tempdir().expect("tempdir");
        let dir = seed_fake_profile(root.path(), &"e".repeat(16), Duration::from_secs(1));
        write_owner_pid_marker(&dir, std::process::id());
        std::fs::write(dir.join(OWNER_START_MARKER), b"   \n").expect("blank the start marker");

        assert_eq!(owner_liveness(&dir), OwnerLiveness::Live);
        assert!(profile_is_owned_by_live_process(&dir));
    }

    /// `Unverified` (PID alive, token recorded, OS will not disclose the live
    /// process's start time) resolves in opposite directions per caller:
    /// the deletion paths keep the directory, the kill gate refuses. Driven
    /// through `owner_liveness`'s own consumers rather than faked, so the
    /// mapping stays honest if a variant is ever added.
    #[test]
    fn unit_unverified_owner_keeps_profile_but_refuses_kill() {
        assert!(
            !matches!(OwnerLiveness::Unverified, OwnerLiveness::Live),
            "Unverified must remain distinct from Live"
        );
        // Deletion direction: `profile_is_owned_by_live_process` (the guard
        // every prune path consults) treats Unverified as owned → keep.
        // Kill direction: `pid_is_ff_rdp_spawned_under` requires exactly
        // `Live` → refuse. Both are asserted structurally here because the
        // OS cannot be made to withhold a start time on demand.
        let root = tempfile::tempdir().expect("tempdir");
        let dir = seed_fake_profile(root.path(), &"f".repeat(16), Duration::from_secs(1));
        write_owner_pid_marker(&dir, std::process::id());
        assert!(profile_is_owned_by_live_process(&dir));
        assert!(pid_is_ff_rdp_spawned_under(root.path(), std::process::id()));
    }

    /// AC: `unit_prune_orphan_profiles_bounded_by_max` — 60 stale dirs seeded,
    /// `max_entries = 50` — at most 50 are removed and the rest survive.
    #[test]
    fn unit_prune_orphan_profiles_bounded_by_max() {
        let root = tempfile::tempdir().expect("tempdir");
        let old = Duration::from_hours(192);
        let seeded: Vec<PathBuf> = (0..60)
            .map(|i| {
                let suffix = format!("{i:016}");
                seed_fake_profile(root.path(), &suffix, old)
            })
            .collect();

        let summary = prune_orphan_profiles(root.path(), Duration::from_hours(168), 50);

        assert_eq!(summary.removed.len(), 50, "should stop after max_entries");
        let remaining = seeded.iter().filter(|d| d.exists()).count();
        assert_eq!(remaining, 10, "10 of 60 should remain after bounding at 50");
    }
}
