//! Consolidated live-Firefox integration-test target for `ff-rdp-cli`.
//!
//! Introduced in iter-100b (live-test binary consolidation). Every live
//! suite that used to be its own top-level `tests/live_*.rs` binary is now
//! a `mod` under `tests/live/`, compiled into this single test target so a
//! plain `cargo test` links one live binary instead of ~45. All tests remain
//! `#[ignore]`-gated behind `FF_RDP_LIVE_TESTS=1` (and network tests behind
//! `FF_RDP_LIVE_NETWORK_TESTS=1`).
//!
//! Run the whole suite against headless Firefox:
//!   FF_RDP_LIVE_TESTS=1 cargo test-live -p ff-rdp-cli --test live
//!
//! Run one migrated suite:
//!   FF_RDP_LIVE_TESTS=1 cargo test -p ff-rdp-cli --test live live_96 -- --include-ignored
//!
//! Enumerate every test name (no Firefox needed):
//!   cargo test -p ff-rdp-cli --test live -- --list
//!
//! New live tests go in `tests/live/<slug>.rs` + a `mod` line below — never a
//! new top-level `tests/live_*.rs` file (enforced by
//! `cargo run -p xtask -- check-live-test-layout`).

// iter-105 Theme D: several live suites call `libc::kill` via FFI for daemon
// process-lifecycle assertions.  The CLI crate default is
// `unsafe_code = "deny"`; allow it crate-wide for this test binary only (every
// block carries its own `// SAFETY:` note) so the FFI-using live suites compile.
#![allow(unsafe_code)]

// The shared live-test helpers live one directory up so the other top-level
// test binaries can keep including them per-file. Declare them once here.
#[path = "../common/mod.rs"]
mod common;

mod live_100_daemon_lifecycle_hardening;
mod live_102_longstring_and_reload;
mod live_103_emulate;
mod live_104_security_pwa;
mod live_61l;
mod live_61q_resource_bus;
mod live_61r_eval;
mod live_61r_screenshot;
mod live_62_page_map_index;
mod live_86_perf_field_fixes;
mod live_90_daemon_lifecycle;
mod live_92_navigate_epoch;
mod live_92_screenshot_full_page;
mod live_94_polish_bundle;
mod live_95_cascade_computed_agreement;
mod live_96_profile_cleanup;
mod live_98_media_query_truthfulness;
mod live_a11y_contrast_wai_bad;
mod live_a11y_critical;
mod live_bulk_cap;
mod live_cascade;
mod live_cascade_explains_pico_dialog;
mod live_cascade_real_site;
mod live_console_no_double_delivery;
mod live_console_printf;
mod live_cookies;
mod live_cookies_set_cookie_header;
mod live_cross_actor;
mod live_daemon_heavy_spa;
mod live_daemon_stop_mdn;
mod live_daemon_watch_targets;
mod live_dom_include_style;
mod live_dom_stats_perf_audit_parity;
mod live_eval_csp;
mod live_eval_scope;
mod live_grip_release;
mod live_navigate_default_fast;
mod live_navigate_readiness;
mod live_navigate_real_site;
mod live_network_default_watcher;
mod live_network_headers;
mod live_oneway;
mod live_perf_vitals_headless;
mod live_reload_hard;
mod live_screenshot_bulk_fallback;
mod live_screenshot_ff151;
mod live_screenshot_shim;
mod live_snapshot_max_depth;
mod live_stale_tab_race;
mod live_styles_applied;
mod live_styles_applied_dedupe;
mod live_target_destroyed;
mod live_wait_timeout_ms_canonical;
