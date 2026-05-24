#![no_main]
//! Fuzz harness for `parse_page_map_str` (page-map JSON/YAML loader).
//!
//! The parser is included via `#[path]` so the fuzz crate stays
//! independent of the `ff-rdp-cli` binary crate's module tree.

use libfuzzer_sys::fuzz_target;

#[allow(dead_code)]
#[path = "../../crates/ff-rdp-cli/src/page_map/mod.rs"]
mod page_map;

fuzz_target!(|data: &[u8]| {
    let Ok(s) = std::str::from_utf8(data) else {
        return;
    };
    // Drive both code paths from one input — alternating chooses based on
    // the first byte to avoid wasting cycles re-fuzzing the same UTF-8 prefix.
    let fmt = if s.as_bytes().first().is_some_and(|b| b % 2 == 0) {
        page_map::PageMapFormat::Json
    } else {
        page_map::PageMapFormat::Yaml
    };
    let _ = page_map::parse_page_map_str(s, fmt);
});
