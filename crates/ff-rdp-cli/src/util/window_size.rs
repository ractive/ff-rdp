//! Shared `WxH` window-size parsing for `launch --window-size` and
//! `screenshot --window-size` (iter-133).
//!
//! Both commands accept the same `WxH` pixel-size syntax but apply it very
//! differently — `launch` forwards it to a live, long-running Firefox
//! instance (subject to the platform's live-viewport floor); `screenshot`
//! forwards it to a one-shot headless-shell `--screenshot` subprocess (no
//! floor, exact PNG dimensions). See kb/research/viewport-emulation.md for
//! the empirical basis.

use crate::error::AppError;

/// The ~500px live-viewport floor observed for a debugger-server headless
/// Firefox instance (`launch`'s mode) on macOS — see
/// `kb/research/viewport-emulation.md`. A `launch --window-size` request
/// narrower than this on the width axis still forwards `-width`/`-height`
/// to Firefox, but the live `innerWidth` is expected to clamp up to the
/// floor rather than honor the smaller requested value.
pub(crate) const LIVE_VIEWPORT_FLOOR_PX: u32 = 500;

/// Parse a `WxH` pixel-size string (e.g. `"390x844"`) into `(width, height)`.
///
/// Rejects: no `x` separator, an empty width or height, non-numeric parts,
/// and a zero width or height — all with a user-facing error naming the
/// expected `WxH` form.
pub(crate) fn parse_window_size(s: &str) -> Result<(u32, u32), AppError> {
    let invalid = || {
        AppError::User(format!(
            "invalid --window-size '{s}': expected WxH form (e.g. 390x844)"
        ))
    };

    let (w_str, h_str) = s.split_once('x').ok_or_else(invalid)?;
    if w_str.is_empty() || h_str.is_empty() {
        return Err(invalid());
    }
    let width: u32 = w_str.parse().map_err(|_| invalid())?;
    let height: u32 = h_str.parse().map_err(|_| invalid())?;
    if width == 0 || height == 0 {
        return Err(AppError::User(format!(
            "invalid --window-size '{s}': width and height must be greater than 0"
        )));
    }
    Ok((width, height))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// AC: unit_launch_window_size_validation — `0x0`, `x`, `390`, `390x` all
    /// rejected with a user error naming the expected `WxH` form.
    #[test]
    fn unit_launch_window_size_validation_rejects_zero_dimensions() {
        let err = parse_window_size("0x0").unwrap_err();
        assert!(
            format!("{err}").contains("WxH") || format!("{err}").contains("greater than 0"),
            "unexpected message: {err}"
        );
    }

    #[test]
    fn unit_launch_window_size_validation_rejects_bare_separator() {
        let err = parse_window_size("x").unwrap_err();
        assert!(
            format!("{err}").contains("WxH"),
            "unexpected message: {err}"
        );
    }

    #[test]
    fn unit_launch_window_size_validation_rejects_missing_separator() {
        let err = parse_window_size("390").unwrap_err();
        assert!(
            format!("{err}").contains("WxH"),
            "unexpected message: {err}"
        );
    }

    #[test]
    fn unit_launch_window_size_validation_rejects_trailing_separator() {
        let err = parse_window_size("390x").unwrap_err();
        assert!(
            format!("{err}").contains("WxH"),
            "unexpected message: {err}"
        );
    }

    #[test]
    fn unit_launch_window_size_validation_rejects_non_numeric() {
        assert!(parse_window_size("abcxdef").is_err());
    }

    #[test]
    fn parse_window_size_accepts_valid_dimensions() {
        assert_eq!(parse_window_size("390x844").unwrap(), (390, 844));
        assert_eq!(parse_window_size("600x800").unwrap(), (600, 800));
    }

    #[test]
    fn live_viewport_floor_is_500() {
        // Pinned so a future edit that changes the constant re-reads the
        // research doc rationale rather than drifting silently.
        assert_eq!(LIVE_VIEWPORT_FLOOR_PX, 500);
    }
}
