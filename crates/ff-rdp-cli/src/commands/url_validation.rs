use crate::error::AppError;

const ALLOWED_SCHEMES: &[&str] = &["http", "https", "file", "about"];

pub fn validate_url(url: &str) -> Result<(), AppError> {
    let colon_pos = url
        .find(':')
        .ok_or_else(|| AppError::User(format!("invalid URL (no scheme): {url}")))?;

    let scheme = url[..colon_pos].to_ascii_lowercase();

    if scheme.is_empty() {
        return Err(AppError::User(format!("invalid URL (empty scheme): {url}")));
    }

    if !ALLOWED_SCHEMES.contains(&scheme.as_str()) {
        return Err(AppError::User(format!(
            "URL scheme '{scheme}:' is not allowed; permitted schemes: http, https, file, about. Use --allow-unsafe-urls to allow javascript: and data: schemes"
        )));
    }

    Ok(())
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
    fn allows_file() {
        assert!(validate_url("file:///tmp/index.html").is_ok());
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
