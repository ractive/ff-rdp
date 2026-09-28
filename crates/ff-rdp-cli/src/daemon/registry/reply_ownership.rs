//! Atomic last-session ownership diagnostics, ordered under the registry lock.
use super::{
    Context, DaemonInfo, Path, Result, fs, generate_auth_token, read_registry_in, registry_dir,
    registry_filename, remove_legacy_registry_in, remove_registry_in,
    try_acquire_registry_write_lock_in,
};
use serde_json::{Value, json};
use std::io::{Read as _, Write as _};

fn receipt_name(port: u16) -> String {
    format!("daemon.{port}.reply-ownership.json")
}
fn sequence_name(port: u16) -> String {
    format!("daemon.{port}.session-sequence.json")
}

fn read_bounded(path: &Path) -> Result<Option<Value>> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    anyhow::ensure!(
        metadata.file_type().is_file() && metadata.len() <= 65536,
        "invalid ownership diagnostic file"
    );
    let mut bytes = Vec::new();
    fs::File::open(path)?.take(65537).read_to_end(&mut bytes)?;
    anyhow::ensure!(bytes.len() <= 65536, "ownership diagnostic exceeds bound");
    Ok(Some(serde_json::from_slice(&bytes)?))
}
fn atomic_json(dir: &Path, name: &str, value: &Value) -> Result<()> {
    let nonce = generate_auth_token()?;
    let temporary = dir.join(format!("{name}.{nonce}.tmp"));
    let mut options = fs::OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temporary)?;
    let result = (|| {
        file.write_all(&serde_json::to_vec(value)?)?;
        file.flush()?;
        drop(file);
        fs::rename(&temporary, dir.join(name))?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}
fn session_identity(sequence: Option<u64>, id: Option<&str>) -> Result<Option<(u64, &str)>> {
    match (sequence, id) {
        (Some(sequence), Some(id)) if sequence > 0 && !id.is_empty() => Ok(Some((sequence, id))),
        (None, None) => Ok(None), // Explicit legacy identity, never ordered as zero.
        _ => anyhow::bail!("invalid or incomplete session identity"),
    }
}
fn live_identity(info: &DaemonInfo, port: u16) -> Result<Option<(u64, &str)>> {
    anyhow::ensure!(info.firefox_port == port, "live session port mismatch");
    session_identity(info.session_sequence, info.session_id.as_deref())
}
fn consistent_identities(a: (u64, &str), b: (u64, &str)) -> Result<()> {
    anyhow::ensure!(
        a.0 != b.0 || a.1 == b.1,
        "session sequence identity conflict"
    );
    Ok(())
}
fn identity(value: &Value, port: u16) -> Result<(u64, &str)> {
    anyhow::ensure!(
        value["schema"] == 1 && value["firefox_port"] == port,
        "unknown ownership receipt schema/port"
    );
    session_identity(
        value["session_sequence"].as_u64(),
        value["session_id"].as_str(),
    )?
    .context("ownership receipt has no session identity")
}
fn validate_snapshot(snapshot: &Value) -> Result<()> {
    let count = |name: &str| {
        snapshot[name]
            .as_u64()
            .with_context(|| format!("invalid ownership counter {name}"))
    };
    let ordinary = count("pending_ordinary")?;
    let asynchronous = count("pending_async")?;
    anyhow::ensure!(
        ordinary.checked_add(asynchronous) == Some(count("pending")?) && count("pending")? <= 4096,
        "inconsistent pending ownership counters"
    );
    anyhow::ensure!(
        count("oneway_hazard_actors")? <= 4096 && count("terminal_actor_count")? <= 4096,
        "invalid bounded actor counters"
    );
    // Older schema-1 receipts predate quarantine and remain readable. Once any
    // partition field is present, require the complete internally consistent set.
    let partition = [
        "active_pending",
        "orphan_pending",
        "orphan_pending_ordinary",
        "orphan_pending_async",
        "quarantined_actor_count",
    ];
    if partition.iter().any(|name| snapshot.get(*name).is_some()) {
        let orphan = count("orphan_pending")?;
        let orphan_ordinary = count("orphan_pending_ordinary")?;
        let orphan_async = count("orphan_pending_async")?;
        anyhow::ensure!(
            count("active_pending")?.checked_add(orphan) == Some(count("pending")?)
                && orphan_ordinary.checked_add(orphan_async) == Some(orphan)
                && orphan_ordinary <= ordinary
                && orphan_async <= asynchronous
                && count("quarantined_actor_count")? <= 4096
                && count("quarantined_actor_count")? <= orphan + count("oneway_hazard_actors")?
                && (orphan == 0 || count("quarantined_actor_count")? != 0),
            "inconsistent orphan ownership counters"
        );
    }
    for name in [
        "departures_with_outstanding",
        "ambiguous_replies",
        "discarded_replies",
    ] {
        count(name)?;
    }
    let retiring = snapshot["retiring"]
        .as_bool()
        .context("missing ownership retirement state")?;
    let reason = &snapshot["terminal_reason"];
    anyhow::ensure!(
        (retiring && reason.as_str().is_some_and(|reason| !reason.is_empty()))
            || (!retiring && reason.is_null()),
        "inconsistent ownership terminal reason"
    );
    anyhow::ensure!(
        snapshot["shutdown_reason"].is_string(),
        "missing shutdown disposition"
    );
    Ok(())
}

pub(crate) fn read_receipt_in(dir: &Path, port: u16) -> Result<Option<Value>> {
    let value = read_bounded(&dir.join(receipt_name(port)))?;
    if let Some(value) = &value {
        identity(value, port)?;
        anyhow::ensure!(
            value["pid"]
                .as_u64()
                .is_some_and(|pid| pid > 0 && u32::try_from(pid).is_ok()),
            "invalid terminal pid"
        );
        anyhow::ensure!(
            value["connection_generation"] == 0
                && value["persistence_status"] == "atomic_normal_termination",
            "unknown receipt disposition"
        );
        chrono::DateTime::parse_from_rfc3339(
            value["ended_at"]
                .as_str()
                .context("missing terminal timestamp")?,
        )?;
        validate_snapshot(&value["reply_ownership"])?;
    }
    Ok(value)
}
pub(crate) fn read_ownership_receipt(port: u16) -> Result<Option<Value>> {
    read_receipt_in(&registry_dir()?, port)
}
pub(crate) fn publish_in(dir: &Path, info: &mut DaemonInfo) -> Result<()> {
    let _ = live_identity(info, info.firefox_port)?;
    fs::create_dir_all(dir)?;
    let _lock = try_acquire_registry_write_lock_in(dir, info.firefox_port)?;
    let previous = read_bounded(&dir.join(sequence_name(info.firefox_port)))?;
    let mut maximum = match previous {
        Some(value) => value
            .as_u64()
            .context("invalid persisted session sequence")?,
        None => 0,
    };
    let live = read_registry_in(dir, info.firefox_port)?;
    let live_id = live
        .as_ref()
        .map(|live| live_identity(live, info.firefox_port))
        .transpose()?
        .flatten();
    let receipt = read_receipt_in(dir, info.firefox_port)?;
    let terminal_id = receipt
        .as_ref()
        .map(|receipt| identity(receipt, info.firefox_port))
        .transpose()?;
    // Validate the pair before allocating or publishing either file. Taking
    // only max(sequence) would silently overwrite a conflicting live identity.
    if let (Some(live_id), Some(terminal_id)) = (live_id, terminal_id) {
        consistent_identities(live_id, terminal_id)?;
    }
    if live.is_some() && live_id.is_none() {
        tracing::info!("replacing legacy registry with unknown session ordering");
    }
    for (sequence, _) in live_id.into_iter().chain(terminal_id) {
        maximum = maximum.max(sequence);
    }
    let sequence = maximum
        .checked_add(1)
        .context("session sequence exhausted")?;
    info.session_sequence = Some(sequence);
    info.session_id = Some(generate_auth_token()?);
    atomic_json(dir, &sequence_name(info.firefox_port), &json!(sequence))?;
    atomic_json(
        dir,
        &registry_filename(info.firefox_port),
        &serde_json::to_value(info)?,
    )
}
pub(crate) fn publish_session(info: &mut DaemonInfo) -> Result<()> {
    let dir = registry_dir()?;
    remove_legacy_registry_in(&dir);
    publish_in(&dir, info)
}

pub(crate) fn write_ownership_receipt_in(
    dir: &Path,
    info: &DaemonInfo,
    snapshot: &Value,
) -> Result<bool> {
    validate_snapshot(snapshot)?;
    let own_id = live_identity(info, info.firefox_port)?
        .context("terminal session has unknown session ordering")?;
    let (sequence, id) = own_id;
    let _lock = try_acquire_registry_write_lock_in(dir, info.firefox_port)?;
    let existing = read_receipt_in(dir, info.firefox_port)?;
    let live = read_registry_in(dir, info.firefox_port)?;
    let terminal_id = existing
        .as_ref()
        .map(|v| identity(v, info.firefox_port))
        .transpose()?;
    // Publication may explicitly replace a legacy live record; finalization
    // cannot establish its ordering and must preserve it without writing.
    let live_id = live
        .as_ref()
        .map(|live| {
            live_identity(live, info.firefox_port)?
                .context("live registry has unknown session ordering")
        })
        .transpose()?;
    if let (Some(live_id), Some(terminal_id)) = (live_id, terminal_id) {
        consistent_identities(live_id, terminal_id)?;
    }
    let identities = [terminal_id, live_id];
    for other in identities.into_iter().flatten() {
        consistent_identities(own_id, other)?;
    }
    if identities
        .into_iter()
        .flatten()
        .any(|other| other.0 > sequence)
    {
        return Ok(false);
    }
    let receipt = json!({"schema":1,"firefox_port":info.firefox_port,"session_sequence":sequence,
        "session_id":id,"pid":info.pid,"start_token":info.start_token,"ended_at":chrono::Utc::now().to_rfc3339(),
        "connection_generation":0,"reply_ownership":snapshot,"persistence_status":"atomic_normal_termination"});
    atomic_json(dir, &receipt_name(info.firefox_port), &receipt)?;
    Ok(true)
}
pub(crate) fn remove_session(info: &DaemonInfo) -> Result<()> {
    let own_id = live_identity(info, info.firefox_port)?
        .context("removing session has unknown session ordering")?;
    let dir = registry_dir()?;
    let _lock = try_acquire_registry_write_lock_in(&dir, info.firefox_port)?;
    if let Some(live) = read_registry_in(&dir, info.firefox_port)? {
        let live_id = live_identity(&live, info.firefox_port)?
            .context("live registry has unknown session ordering")?;
        consistent_identities(own_id, live_id)?;
        if live_id == own_id && live.pid == info.pid && live.start_token == info.start_token {
            remove_registry_in(&dir, info.firefox_port)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::write_registry_in;
    use super::*;
    use std::path::PathBuf;
    fn info(port: u16) -> DaemonInfo {
        DaemonInfo {
            session_sequence: None,
            session_id: None,
            pid: 1,
            proxy_port: 1,
            firefox_host: "localhost".into(),
            firefox_port: port,
            started_at: "test".into(),
            auth_token: "test".into(),
            start_token: Some("birth".into()),
        }
    }
    fn snapshot() -> Value {
        let mut value = ff_rdp_core::ReplyAccounting::default().snapshot();
        value["departures_with_outstanding"] = json!(0);
        value["shutdown_reason"] = json!("None");
        value
    }
    fn existing_bytes(dir: &Path) -> std::collections::BTreeMap<PathBuf, Vec<u8>> {
        fs::read_dir(dir)
            .unwrap()
            .map(|entry| {
                let path = entry.unwrap().path();
                (path.file_name().unwrap().into(), fs::read(path).unwrap())
            })
            .collect()
    }
    #[test]
    fn older_finalizer_cannot_replace_newer_ended_session() {
        let dir = tempfile::tempdir().unwrap();
        let mut a = info(6000);
        let mut b = info(6000);
        publish_in(dir.path(), &mut a).unwrap();
        publish_in(dir.path(), &mut b).unwrap();
        assert!(write_ownership_receipt_in(dir.path(), &b, &snapshot()).unwrap());
        remove_registry_in(dir.path(), 6000).unwrap();
        let before = existing_bytes(dir.path());
        assert!(!write_ownership_receipt_in(dir.path(), &a, &snapshot()).unwrap());
        assert_eq!(existing_bytes(dir.path()), before);
        assert_eq!(
            read_receipt_in(dir.path(), 6000).unwrap().unwrap()["session_id"],
            b.session_id.unwrap()
        );
    }
    #[test]
    fn same_sequence_different_identity_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let mut a = info(6000);
        publish_in(dir.path(), &mut a).unwrap();
        let mut conflict = a.clone();
        conflict.session_id = Some("different".into());
        let before = existing_bytes(dir.path());
        assert!(write_ownership_receipt_in(dir.path(), &conflict, &snapshot()).is_err());
        assert_eq!(existing_bytes(dir.path()), before);
    }
    #[test]
    fn publication_rejects_live_terminal_identity_conflict_without_any_write() {
        let dir = tempfile::tempdir().unwrap();
        let mut a = info(6000);
        publish_in(dir.path(), &mut a).unwrap();
        assert!(write_ownership_receipt_in(dir.path(), &a, &snapshot()).unwrap());
        let mut conflict = a.clone();
        conflict.session_id = Some("different".into());
        write_registry_in(dir.path(), &conflict).unwrap();
        let before = existing_bytes(dir.path());
        let mut next = info(6000);
        let next_before = serde_json::to_value(&next).unwrap();
        let error = publish_in(dir.path(), &mut next).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("session sequence identity conflict")
        );
        assert_eq!(existing_bytes(dir.path()), before);
        assert_eq!(serde_json::to_value(&next).unwrap(), next_before);
        // Even a newer finalizer must validate the conflicting pair first.
        let mut newer = a.clone();
        newer.session_sequence = Some(a.session_sequence.unwrap() + 1);
        assert!(write_ownership_receipt_in(dir.path(), &newer, &snapshot()).is_err());
        assert_eq!(existing_bytes(dir.path()), before);
    }
    #[test]
    fn malformed_live_identity_cannot_be_finalized_or_published_over() {
        let dir = tempfile::tempdir().unwrap();
        let mut a = info(6000);
        publish_in(dir.path(), &mut a).unwrap();
        assert!(write_ownership_receipt_in(dir.path(), &a, &snapshot()).unwrap());
        for (sequence, id) in [
            (Some(0), Some("present")),
            (Some(1), Some("")),
            (Some(1), None),
            (None, Some("present")),
        ] {
            let mut malformed = a.clone();
            malformed.session_sequence = sequence;
            malformed.session_id = id.map(str::to_owned);
            write_registry_in(dir.path(), &malformed).unwrap();
            let before = existing_bytes(dir.path());
            assert!(write_ownership_receipt_in(dir.path(), &a, &snapshot()).is_err());
            assert_eq!(existing_bytes(dir.path()), before);
            let mut next = info(6000);
            assert!(publish_in(dir.path(), &mut next).is_err());
            assert_eq!(existing_bytes(dir.path()), before);
            // The finalizer's own identity is subject to the same validation.
            assert!(write_ownership_receipt_in(dir.path(), &malformed, &snapshot()).is_err());
            assert_eq!(existing_bytes(dir.path()), before);
        }
    }
    #[test]
    fn malformed_terminal_identity_preserves_all_existing_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let mut a = info(6000);
        publish_in(dir.path(), &mut a).unwrap();
        assert!(write_ownership_receipt_in(dir.path(), &a, &snapshot()).unwrap());
        let valid = read_receipt_in(dir.path(), 6000).unwrap().unwrap();
        for (sequence, id) in [
            (json!(0), json!("present")),
            (json!(1), json!("")),
            (json!(1), Value::Null),
            (Value::Null, json!("present")),
            (Value::Null, Value::Null),
        ] {
            let mut malformed = valid.clone();
            malformed["session_sequence"] = sequence;
            malformed["session_id"] = id;
            fs::write(
                dir.path().join(receipt_name(6000)),
                serde_json::to_vec(&malformed).unwrap(),
            )
            .unwrap();
            let before = existing_bytes(dir.path());
            assert!(read_receipt_in(dir.path(), 6000).is_err());
            assert!(write_ownership_receipt_in(dir.path(), &a, &snapshot()).is_err());
            assert_eq!(existing_bytes(dir.path()), before);
            let mut next = info(6000);
            assert!(publish_in(dir.path(), &mut next).is_err());
            assert_eq!(existing_bytes(dir.path()), before);
        }
    }
    #[test]
    fn legacy_live_identity_is_explicitly_unordered_until_publication() {
        let dir = tempfile::tempdir().unwrap();
        let mut a = info(6000);
        publish_in(dir.path(), &mut a).unwrap();
        assert!(write_ownership_receipt_in(dir.path(), &a, &snapshot()).unwrap());
        write_registry_in(dir.path(), &info(6000)).unwrap();
        let before = existing_bytes(dir.path());
        assert!(write_ownership_receipt_in(dir.path(), &a, &snapshot()).is_err());
        assert_eq!(existing_bytes(dir.path()), before);
        let mut next = info(6000);
        publish_in(dir.path(), &mut next).unwrap();
        assert!(next.session_sequence > a.session_sequence);
        assert_eq!(
            fs::read(dir.path().join(receipt_name(6000))).unwrap(),
            before[Path::new(&receipt_name(6000))]
        );
    }
    #[test]
    fn corrupt_receipt_is_visible_and_not_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let mut a = info(6000);
        publish_in(dir.path(), &mut a).unwrap();
        fs::write(dir.path().join(receipt_name(6000)), b"{}").unwrap();
        assert!(read_receipt_in(dir.path(), 6000).is_err());
        assert!(write_ownership_receipt_in(dir.path(), &a, &snapshot()).is_err());
        let malformed = json!({"schema":1,"firefox_port":6000,"session_sequence":a.session_sequence,"session_id":a.session_id,"pid":1,"connection_generation":0,"persistence_status":"atomic_normal_termination","ended_at":chrono::Utc::now().to_rfc3339(),"reply_ownership":{}});
        fs::write(dir.path().join(receipt_name(6000)), malformed.to_string()).unwrap();
        assert!(read_receipt_in(dir.path(), 6000).is_err());
    }
    #[test]
    fn ownership_lock_contention_is_reported_without_wait_or_replacement() {
        let dir = tempfile::tempdir().unwrap();
        let mut a = info(6000);
        publish_in(dir.path(), &mut a).unwrap();
        write_ownership_receipt_in(dir.path(), &a, &snapshot()).unwrap();
        let before = fs::read(dir.path().join(receipt_name(6000))).unwrap();
        let _held = super::super::acquire_registry_write_lock_in(dir.path(), 6000).unwrap();
        let started = std::time::Instant::now();
        assert!(write_ownership_receipt_in(dir.path(), &a, &snapshot()).is_err());
        assert!(started.elapsed() < std::time::Duration::from_secs(1));
        assert_eq!(
            fs::read(dir.path().join(receipt_name(6000))).unwrap(),
            before
        );
    }
    #[test]
    fn orphan_partition_is_persisted_and_invalid_partition_preserves_receipt() {
        let dir = tempfile::tempdir().unwrap();
        let mut info = info(6000);
        publish_in(dir.path(), &mut info).unwrap();
        let mut accounting = ff_rdp_core::ReplyAccounting::default();
        accounting
            .register("old", ff_rdp_core::ReplyContract::Ordinary)
            .unwrap();
        accounting.quarantine_current();
        accounting
            .register("current", ff_rdp_core::ReplyContract::AsyncEvaluation)
            .unwrap();
        let mut value = accounting.snapshot();
        value["departures_with_outstanding"] = json!(1);
        value["shutdown_reason"] = json!("AuthenticatedShutdown");
        assert!(write_ownership_receipt_in(dir.path(), &info, &value).unwrap());
        let persisted = read_receipt_in(dir.path(), 6000).unwrap().unwrap();
        assert_eq!(persisted["reply_ownership"]["pending"], 2);
        assert_eq!(persisted["reply_ownership"]["active_pending"], 1);
        assert_eq!(persisted["reply_ownership"]["orphan_pending_ordinary"], 1);
        let before = existing_bytes(dir.path());
        for field in [
            "active_pending",
            "orphan_pending",
            "orphan_pending_ordinary",
            "orphan_pending_async",
            "quarantined_actor_count",
        ] {
            let mut invalid = value.clone();
            invalid.as_object_mut().unwrap().remove(field);
            assert!(write_ownership_receipt_in(dir.path(), &info, &invalid).is_err());
            assert_eq!(existing_bytes(dir.path()), before);
        }
        for (field, invalid_count) in [
            ("active_pending", 2),
            ("orphan_pending", 2),
            ("orphan_pending_ordinary", 0),
            ("orphan_pending_async", 1),
            ("quarantined_actor_count", 0),
            ("quarantined_actor_count", 2),
            ("quarantined_actor_count", 4097),
        ] {
            let mut invalid = value.clone();
            invalid[field] = json!(invalid_count);
            assert!(write_ownership_receipt_in(dir.path(), &info, &invalid).is_err());
            assert_eq!(existing_bytes(dir.path()), before);
        }
    }

    #[test]
    fn historical_receipt_without_partition_remains_readable_without_invented_zeroes() {
        let mut legacy = snapshot();
        for field in [
            "active_pending",
            "orphan_pending",
            "orphan_pending_ordinary",
            "orphan_pending_async",
            "quarantined_actor_count",
        ] {
            legacy.as_object_mut().unwrap().remove(field);
        }
        assert!(validate_snapshot(&legacy).is_ok());
        assert!(legacy.get("orphan_pending").is_none());
    }
}
