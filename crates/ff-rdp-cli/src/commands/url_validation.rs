//! URL scheme allow-listing for navigation commands.
//!
//! # Threat model
//!
//! `file://` URLs are accepted only behind the explicit `--allow-file-urls`
//! flag. The risk is information disclosure: once Firefox loads
//! `file:///etc/passwd` (or any local file readable by the user running the
//! browser), downstream commands such as `page-text`, `eval`, and
//! `screenshot` can exfiltrate the contents. The same window also opens up
//! via a navigation that begins with `http(s)` and 30x-redirects to a
//! `file://` URL, so the validator runs on every `--url` argument, not just
//! interactive input.
//!
//! `javascript:` and `data:` remain behind `--allow-unsafe-urls` (XSS /
//! arbitrary-content vectors); `vbscript:` and other schemes are always
//! rejected.

use crate::error::AppError;

const ALLOWED_SCHEMES: &[&str] = &["http", "https", "about"];
const FILE_SCHEME: &str = "file";

/// Validate that `url`'s scheme is on the allow list.
///
/// `allow_file_urls` opts into `file://` URLs (off by default — see the
/// module-level threat model).
pub fn validate_url_with_opts(url: &str, allow_file_urls: bool) -> Result<(), AppError> {
    let colon_pos = url
        .find(':')
        .ok_or_else(|| AppError::User(format!("invalid URL (no scheme): {url}")))?;

    let scheme = url[..colon_pos].to_ascii_lowercase();

    if scheme.is_empty() {
        return Err(AppError::User(format!("invalid URL (empty scheme): {url}")));
    }

    if scheme == FILE_SCHEME {
        if allow_file_urls {
            return Ok(());
        }
        return Err(AppError::User(
            "URL scheme 'file:' is not allowed by default; pass --allow-file-urls to opt in (exfiltrates local files via subsequent page-text/eval/screenshot)".to_string(),
        ));
    }

    if !ALLOWED_SCHEMES.contains(&scheme.as_str()) {
        return Err(AppError::User(format!(
            "URL scheme '{scheme}:' is not allowed; permitted schemes: http, https, about. Use --allow-file-urls for file:, or --allow-unsafe-urls for javascript:/data:"
        )));
    }

    Ok(())
}

/// Back-compat wrapper for call sites that don't gate `file://`.
#[cfg(test)]
fn validate_url(url: &str) -> Result<(), AppError> {
    validate_url_with_opts(url, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_http() {
        assert!(validate_url("http://example.com").is_ok());
    }

    #[test]
    fn allows_https() {
        assert!(validate_url("https://example.com/path?q=1").is_ok());
    }

    #[test]
    fn rejects_file_by_default() {
        let err = validate_url("file:///tmp/index.html").unwrap_err();
        assert!(matches!(err, AppError::User(_)));
        assert!(err.to_string().contains("--allow-file-urls"));
    }

    #[test]
    fn allows_file_when_opted_in() {
        assert!(validate_url_with_opts("file:///tmp/index.html", true).is_ok());
    }

    #[test]
    fn allows_about_blank() {
        assert!(validate_url("about:blank").is_ok());
    }

    #[test]
    fn allows_about_newtab() {
        assert!(validate_url("about:newtab").is_ok());
    }

    #[test]
    fn rejects_javascript() {
        let err = validate_url("javascript:alert(1)").unwrap_err();
        assert!(matches!(err, AppError::User(_)));
        assert!(err.to_string().contains("javascript"));
    }

    #[test]
    fn rejects_data() {
        let err = validate_url("data:text/html,<h1>hi</h1>").unwrap_err();
        assert!(matches!(err, AppError::User(_)));
        assert!(err.to_string().contains("data"));
    }

    #[test]
    fn rejects_vbscript() {
        let err = validate_url("vbscript:MsgBox(1)").unwrap_err();
        assert!(matches!(err, AppError::User(_)));
    }

    #[test]
    fn rejects_no_scheme() {
        let err = validate_url("example.com").unwrap_err();
        assert!(matches!(err, AppError::User(_)));
    }

    #[test]
    fn scheme_comparison_is_case_insensitive() {
        assert!(validate_url("HTTP://example.com").is_ok());
        assert!(validate_url("HTTPS://example.com").is_ok());
        let err = validate_url("Javascript:alert(1)").unwrap_err();
        assert!(matches!(err, AppError::User(_)));
        // File scheme is case-insensitive too.
        let err = validate_url("FILE:///tmp/x").unwrap_err();
        assert!(err.to_string().contains("--allow-file-urls"));
        assert!(validate_url_with_opts("FILE:///tmp/x", true).is_ok());
    }

    #[test]
    fn rejects_empty_scheme() {
        let err = validate_url(":foo").unwrap_err();
        assert!(err.to_string().contains("empty scheme"));
    }

    #[test]
    fn rejects_leading_whitespace() {
        let err = validate_url(" http://example.com").unwrap_err();
        assert!(matches!(err, AppError::User(_)));
    }

    #[test]
    fn rejects_empty_string() {
        let err = validate_url("").unwrap_err();
        assert!(matches!(err, AppError::User(_)));
    }
}
