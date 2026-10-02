use super::*;

#[test]
fn unit_147_restart_listener_accepts_actual_lsof_fields() {
    // Exact successful -nP -iTCP:<port> -sTCP:LISTEN -Fp captures:
    // parent-resource-native3/occurrence/003-listener.stdout (p66642/f29),
    // runner-draft/named-native2/occurrence/003-listener.stdout (p88366/f28).
    for (text, expected) in [
        ("p66642\nf29\n", 66642),
        ("p88366\nf28\n", 88366),
        ("p42\nf0\nf29\n", 42),
        ("p42\n", 42),
        ("p42\nf29\np42\nf30\n", 42),
    ] {
        assert_eq!(parse_listener(Some(0), text).unwrap(), Some(expected));
    }
    assert_eq!(parse_listener(Some(1), "").unwrap(), None);
    assert_eq!(parse_listener(Some(1), " \n").unwrap(), None);
}

#[test]
fn unit_147_restart_listener_rejects_malformed_fields() {
    for text in [
        "",
        "\n",
        "f29\n",
        "p42\nx1\n",
        "p42\n\n",
        "p\n",
        "p0\n",
        "p+42\n",
        "p-42\n",
        "p42x\n",
        "p4294967296\n",
        "p42\nf\n",
        "p42\nf-1\n",
        "p42\nf+1\n",
        "p42\nf29u\n",
        "p42\nf4294967296\n",
        "p42\nf 29\n",
    ] {
        assert!(parse_listener(Some(0), text).is_err(), "accepted {text:?}");
    }
    for code in [Some(1), Some(2), None] {
        assert!(parse_listener(code, "p42\nf29\n").is_err());
    }
    assert!(parse_listener(Some(2), "").is_err());
    assert!(parse_listener(None, "").is_err());
}

#[test]
fn unit_147_restart_listener_requires_one_distinct_owner() {
    for text in ["p42\np43\n", "p42\nf29\np43\nf30\n"] {
        assert!(parse_listener(Some(0), text).is_err(), "accepted {text:?}");
    }
}

fn fixture() -> (tempfile::TempDir, Pack, Runtime) {
    let root = tempfile::tempdir().unwrap();
    let path = root.path();
    let xpi = path.join("source.xpi");
    fs::write(
        &xpi,
        english_language_pack::fixture(&english_language_pack::fixture_manifest(), &[]),
    )
    .unwrap();
    let pack = Pack::read(&xpi).unwrap();
    fs::write(path.join("user.js"), USER_JS).unwrap();
    pack.stage(path, USER_JS).unwrap();
    let binary = path.join("firefox");
    fs::write(&binary, b"fixture executable bytes, never executed").unwrap();
    fs::write(
        path.join("application.ini"),
        "[App]\nVersion=156.0.1\nBuildID=20260921121718\n",
    )
    .unwrap();
    let runtime = Runtime::read(&binary).unwrap();
    (root, pack, runtime)
}

fn ready(profile: &Path, pack: &Pack, runtime: &Runtime) -> Value {
    let root = expected_root(profile).unwrap();
    json!({"schema":1,"pid":42,"build":runtime.build,"version":runtime.version,"ready":true,
        "id":english_language_pack::ID,"pack_version":pack.version(),"type":"locale","scope":1,
        "active":true,"user_disabled":false,"app_disabled":false,"signed":2,
        "root":root,"dom":format!("{root}{DOM_MEMBER}")})
}

#[test]
fn unit_147_restart_readiness_requires_owned_signed_provider() {
    let (root, pack, runtime) = fixture();
    let value = ready(root.path(), &pack, &runtime);
    qualify_ready(&value, 42, root.path(), &pack, &runtime).unwrap();
    for (field, bad) in [
        ("pid", json!(43)),
        ("build", json!("wrong")),
        ("ready", json!(false)),
        ("id", json!("other")),
        ("pack_version", json!("1")),
        ("type", json!("extension")),
        ("scope", json!(2)),
        ("signed", json!(0)),
        ("active", json!(false)),
        ("user_disabled", json!(true)),
        ("app_disabled", json!(true)),
        ("root", json!("jar:file:///unowned.xpi!/")),
        ("dom", json!("jar:file:///de.xpi!/dom.properties")),
    ] {
        let mut bad_value = value.clone();
        bad_value[field] = bad;
        assert!(
            qualify_ready(&bad_value, 42, root.path(), &pack, &runtime).is_err(),
            "accepted {field}"
        );
    }
}

#[test]
fn unit_147_restart_persistence_and_immutable_mode_fail_closed() {
    let (root, pack, runtime) = fixture();
    let p = root.path();
    let a = json!({"id":english_language_pack::ID,"version":pack.version(),"type":"locale",
        "location":"app-profile","path":p.join("extensions").join(format!("{}.xpi",english_language_pack::ID)),
        "active":true,"visible":true,"userDisabled":false,"appDisabled":false,"signedState":2,
        "startupData":{"chromeEntries":[["locale","global","en-US","chrome/en-US/locale/en-US/global/"]]}});
    fs::write(
        p.join("extensions.json"),
        serde_json::to_vec(&json!({"addons":[a.clone()]})).unwrap(),
    )
    .unwrap();
    assert!(persisted(p, &pack).is_err());
    fs::write(
        p.join("addonStartup.json.lz4"),
        b"test-only opaque bytes: no contents claim",
    )
    .unwrap();
    let receipt = persisted(p, &pack).unwrap();
    assert_eq!(receipt["startup_cache_contents_attested"], false);
    for field in ["active", "visible", "signedState", "startupData"] {
        let mut bad = a.clone();
        bad[field] = Value::Null;
        fs::write(
            p.join("extensions.json"),
            serde_json::to_vec(&json!({"addons":[bad]})).unwrap(),
        )
        .unwrap();
        assert!(persisted(p, &pack).is_err(), "accepted {field}");
    }
    pack.verify_staged(p, USER_JS).unwrap();
    fs::write(p.join("user.js"), USER_JS).unwrap();
    assert!(pack.verify_staged(p, USER_JS).is_err());
    fs::write(
        p.join("user.js"),
        format!("{USER_JS}{}", english_language_pack::PROFILE_ACTIVATION),
    )
    .unwrap();
    fs::write(
        p.join("extensions")
            .join(format!("{}.xpi", english_language_pack::ID)),
        b"changed",
    )
    .unwrap();
    assert!(pack.verify_staged(p, USER_JS).is_err());
    runtime.verify().unwrap();
    fs::write(&runtime.binary, b"changed").unwrap();
    assert!(runtime.verify().is_err());
}

#[test]
fn unit_147_restart_quit_distinguishes_eof_from_failure() {
    use ff_rdp_core::error::ProtocolError;
    qualify_quit(Ok(json!({"accepted":true}))).unwrap();
    assert!(qualify_quit(Ok(json!({"accepted":false}))).is_err());
    assert!(qualify_quit(Ok(json!({}))).is_err());
    let eof = ProtocolError::RecvFailed(std::io::Error::from(std::io::ErrorKind::UnexpectedEof));
    assert_eq!(
        qualify_quit(Err(eof.into())).unwrap()["acknowledged"],
        false
    );
    for error in [
        ProtocolError::Timeout,
        ProtocolError::InvalidPacket("bad".into()),
        ProtocolError::SendFailed(std::io::Error::from(std::io::ErrorKind::BrokenPipe)),
        ProtocolError::RecvFailed(std::io::Error::from(std::io::ErrorKind::TimedOut)),
    ] {
        assert!(qualify_quit(Err(error.into())).is_err());
    }
    assert!(qualify_quit(Err(anyhow::anyhow!("JavaScript exception"))).is_err());
}

#[cfg(unix)]
#[test]
fn unit_147_restart_actual_child_wait_and_bounded_failure() {
    let mut success = Command::new("/bin/sh")
        .args(["-c", "exit 0"])
        .spawn()
        .unwrap();
    assert!(
        wait_child(&mut success, Instant::now() + Duration::from_secs(2), None)
            .unwrap()
            .success()
    );
    let mut child = Command::new("/bin/sleep").arg("30").spawn().unwrap();
    assert!(wait_child(&mut child, Instant::now(), None).is_err());
    let mut owned = Initializer {
        child,
        stderr: startup::StderrCapture::default(),
        armed: true,
    };
    let error = owned.fail(
        AppError::User("deliberate deadline failure".into()),
        Instant::now() + Duration::from_secs(2),
    );
    assert!(error.to_string().contains("deliberate deadline failure"));
    assert_ne!(owned.child.wait().unwrap().code(), Some(0));
    assert!(!owned.armed);
}

#[test]
fn unit_147_restart_command_and_process_evidence_boundaries() {
    let default = super::super::browser_command(Path::new("firefox"), 7350, false, None, None);
    let init =
        super::super::browser_command(Path::new("firefox"), 7350, true, None, Some("about:blank"));
    let final_cmd = super::super::browser_command(
        Path::new("firefox"),
        7350,
        false,
        Some((800, 600)),
        Some("https://example.invalid/final"),
    );
    let strings = |cmd: &Command| {
        cmd.get_args()
            .map(|s| s.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
    };
    let a = strings(&default);
    let b = strings(&init);
    let c = strings(&final_cmd);
    assert!(!a.contains(&"--headless".into()) && !a.contains(&"--url".into()));
    assert!(b.contains(&"--headless".into()) && b.contains(&"about:blank".into()));
    assert!(!b.contains(&"https://example.invalid/final".into()));
    assert!(
        c.contains(&"https://example.invalid/final".into()) && !c.contains(&"about:blank".into())
    );
    assert!(parse_processes("").is_err());
    assert!(parse_processes("1 2\n").is_err());
    assert!(parse_processes("1 0 1\n1 0 1\n").is_err());
    let rows = parse_processes("42 1 42\n43 42 42\n44 43 44\n90 1 90\n").unwrap();
    assert!(is_descendant(&rows, 44, 42));
    assert!(!is_descendant(&rows, 90, 42));
}
