//! Color helper assertions, included by iter_164_harness_daemon_poll only.

use crate::common::{assert_colors_equal, parse_css_color};

#[test]
fn keyword_matches_rgb_both_directions() {
    assert_eq!(parse_css_color("red"), parse_css_color("rgb(255, 0, 0)"));
    assert_eq!(parse_css_color("rgb(255, 0, 0)"), parse_css_color("red"));
    assert_eq!(parse_css_color("blue"), parse_css_color("rgb(0, 0, 255)"));
    assert_eq!(parse_css_color("rgb(0, 0, 255)"), parse_css_color("blue"));
}

#[test]
fn hex_matches_rgb_both_directions() {
    assert_eq!(
        parse_css_color("#ff0000"),
        parse_css_color("rgb(255, 0, 0)")
    );
    assert_eq!(
        parse_css_color("rgb(255, 0, 0)"),
        parse_css_color("#ff0000")
    );
    assert_eq!(parse_css_color("#f00"), parse_css_color("rgb(255, 0, 0)"));
    assert_eq!(
        parse_css_color("rgb(0, 128, 0)"),
        parse_css_color("#008000")
    );
}

#[test]
fn keyword_matches_hex_both_directions() {
    assert_eq!(parse_css_color("red"), parse_css_color("#ff0000"));
    assert_eq!(parse_css_color("#ff0000"), parse_css_color("red"));
    assert_eq!(parse_css_color("white"), parse_css_color("#fff"));
    assert_eq!(parse_css_color("#fff"), parse_css_color("white"));
}

#[test]
fn rgba_alpha_channel_parses() {
    assert_eq!(
        parse_css_color("rgba(255, 0, 0, 1)"),
        Some((255, 0, 0, 255))
    );
    assert_eq!(parse_css_color("rgba(255, 0, 0, 0)"), Some((255, 0, 0, 0)));
    assert_eq!(parse_css_color("rgba(0, 0, 0, 0.5)"), Some((0, 0, 0, 128)));
}

#[test]
fn eight_digit_hex_matches_rgba() {
    assert_eq!(
        parse_css_color("#ff000080"),
        parse_css_color("rgba(255, 0, 0, 0.5)")
    );
}

#[test]
fn unrecognized_input_returns_none() {
    assert_eq!(parse_css_color("currentcolor"), None);
    assert_eq!(parse_css_color("not-a-color"), None);
}

#[test]
fn assert_colors_equal_passes_for_equivalent_forms() {
    assert_colors_equal("red", "rgb(255, 0, 0)", "test context");
    assert_colors_equal("rgb(0, 0, 255)", "blue", "test context");
}

#[test]
#[should_panic(expected = "color mismatch")]
fn assert_colors_equal_panics_for_different_colors() {
    assert_colors_equal("red", "blue", "test context");
}
