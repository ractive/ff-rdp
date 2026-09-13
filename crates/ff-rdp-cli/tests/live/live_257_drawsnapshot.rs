//! Directly exercise the compatibility fallback, independently of CLI routing.
use std::{process::Command, time::Duration};

use crate::common::{LiveFirefox, base_args, ff_rdp_bin, live_tests_enabled, output_note};
use ff_rdp_core::{RdpTransport, RootActor, ScreenshotActor};

#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_257_drawsnapshot_renders_both_argument_modes_without_changing_scroll() {
    if !live_tests_enabled() {
        return;
    }
    let ff = LiveFirefox::headless_on_random_port();
    let run = |args: &[&str]| {
        let out = Command::new(ff_rdp_bin())
            .args(base_args(ff.port()))
            .args(args)
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", output_note(&out));
        serde_json::from_slice::<serde_json::Value>(&out.stdout).unwrap()["results"].clone()
    };
    run(&[
        "navigate",
        "--allow-unsafe-urls",
        "data:text/html,<body style='margin:0;height:4000px;background:linear-gradient(red,blue)'>iter257</body>",
    ]);
    run(&["eval", "window.scrollTo(0, 500)"]);
    let before = run(&[
        "eval",
        "JSON.stringify({y:scrollY,w:innerWidth,h:innerHeight})",
    ]);
    let metrics: serde_json::Value =
        serde_json::from_str(before.as_str().expect("eval returns the string directly")).unwrap();
    assert_eq!(metrics["y"], 500);
    let mut transport =
        RdpTransport::connect("127.0.0.1", ff.port(), Duration::from_secs(30)).unwrap();
    let tabs = RootActor::list_tabs(&mut transport).unwrap();
    let tab = tabs.iter().find(|tab| tab.selected).unwrap();
    let bc = tab.browsing_context_id.unwrap();
    for full_page in [false, true] {
        let rect = full_page.then(|| (metrics["w"].as_f64().unwrap(), 4000.0));
        let bytes = ScreenshotActor::screenshot_via_process_drawsnapshot(
            &mut transport,
            bc,
            full_page,
            rect,
        )
        .unwrap_or_else(|error| panic!("drawSnapshot must render, full_page={full_page}: {error}"));
        assert!(
            bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
            "capture must be a PNG"
        );
        let height = u32::from_be_bytes(bytes[20..24].try_into().unwrap());
        if full_page {
            assert_eq!(height, 4000);
            assert!(u64::from(height) > metrics["h"].as_u64().unwrap());
        } else {
            assert_eq!(u64::from(height), metrics["h"].as_u64().unwrap());
        }
    }
    drop(transport);
    assert_eq!(
        run(&[
            "eval",
            "JSON.stringify({y:scrollY,w:innerWidth,h:innerHeight})"
        ]),
        before
    );
}
