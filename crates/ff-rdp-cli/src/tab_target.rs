use ff_rdp_core::TabInfo;

use crate::error::AppError;

/// Resolve a `--tab` / `--tab-id` flag pair to a single [`TabInfo`] reference.
///
/// Resolution order:
/// 1. `tab_id` — exact match on `TabInfo::actor`.
/// 2. `tab` as a 1-based integer index into `tabs`.
/// 3. `tab` as a case-insensitive URL substring.
/// 4. Neither flag — first selected tab, falling back to `tabs[0]`.
pub fn resolve_tab<'a>(
    tabs: &'a [TabInfo],
    tab: Option<&str>,
    tab_id: Option<&str>,
) -> Result<&'a TabInfo, AppError> {
    if let Some(id) = tab_id {
        return tabs.iter().find(|t| t.actor.as_ref() == id).ok_or_else(|| {
            AppError::User(format!(
                "no tab with actor ID '{id}'; use `ff-rdp tabs` to list available tabs"
            ))
        });
    }

    if let Some(selector) = tab {
        // Try 1-based integer index first.
        if let Ok(n) = selector.parse::<usize>() {
            let count = tabs.len();
            return if count == 0 {
                Err(AppError::User(
                    "no tabs available — use `ff-rdp launch --headless --temp-profile` to start Firefox, then `ff-rdp tabs`".to_owned(),
                ))
            } else if n == 0 || n > count {
                Err(AppError::User(format!(
                    "tab index {n} out of range (1–{count} tabs available); use `ff-rdp tabs` to list available tabs"
                )))
            } else {
                Ok(&tabs[n - 1])
            };
        }

        // Fall back to case-insensitive URL substring match.
        let lower = selector.to_lowercase();
        return tabs
            .iter()
            .find(|t| t.url.to_lowercase().contains(&lower))
            .ok_or_else(|| AppError::User(format!("no tab matching URL pattern '{selector}'; use `ff-rdp tabs` to list available tabs")));
    }

    // No flag — prefer the selected tab, then the first tab.
    if tabs.is_empty() {
        return Err(AppError::User(
            "no tabs available — is a page open in Firefox? Use `ff-rdp launch --headless --temp-profile` to start one".to_owned(),
        ));
    }

    Ok(tabs.iter().find(|t| t.selected).unwrap_or(&tabs[0]))
}

#[cfg(test)]
mod tests {
    use ff_rdp_core::types::ActorId;

    use super::*;

    fn make_tab(actor: &str, url: &str, selected: bool) -> TabInfo {
        TabInfo {
            actor: ActorId::from(actor),
            title: String::new(),
            url: url.to_owned(),
            selected,
            browsing_context_id: None,
        }
    }

    fn tabs() -> Vec<TabInfo> {
        vec![
            make_tab("server1.conn0.tab1", "https://github.com/rust-lang", false),
            make_tab("server1.conn0.tab2", "https://crates.io", true),
            make_tab("server1.conn0.tab3", "https://docs.rs/tokio", false),
        ]
    }

    // --- --tab-id ---

    #[test]
    fn tab_id_exact_match() {
        let ts = tabs();
        let result = resolve_tab(&ts, None, Some("server1.conn0.tab2")).unwrap();
        assert_eq!(result.actor.as_ref(), "server1.conn0.tab2");
    }

    #[test]
    fn tab_id_not_found_returns_error() {
        let ts = tabs();
        let err = resolve_tab(&ts, None, Some("server1.conn0.tab99")).unwrap_err();
        match err {
            AppError::User(msg) => {
                assert!(msg.contains("server1.conn0.tab99"), "message: {msg}");
            }
            other => panic!("expected AppError::User, got: {other}"),
        }
    }

    // --- --tab as index ---

    #[test]
    fn tab_index_first() {
        let ts = tabs();
        let result = resolve_tab(&ts, Some("1"), None).unwrap();
        assert_eq!(result.actor.as_ref(), "server1.conn0.tab1");
    }

    #[test]
    fn tab_index_last() {
        let ts = tabs();
        let result = resolve_tab(&ts, Some("3"), None).unwrap();
        assert_eq!(result.actor.as_ref(), "server1.conn0.tab3");
    }

    #[test]
    fn tab_index_out_of_range() {
        let ts = tabs();
        let err = resolve_tab(&ts, Some("5"), None).unwrap_err();
        match err {
            AppError::User(msg) => {
                assert!(msg.contains('5'), "message: {msg}");
                assert!(msg.contains('3'), "message: {msg}");
            }
            other => panic!("expected AppError::User, got: {other}"),
        }
    }

    #[test]
    fn tab_index_zero_is_out_of_range() {
        let ts = tabs();
        let err = resolve_tab(&ts, Some("0"), None).unwrap_err();
        match err {
            AppError::User(msg) => assert!(msg.contains('0'), "message: {msg}"),
            other => panic!("expected AppError::User, got: {other}"),
        }
    }

    // --- --tab as URL substring ---

    #[test]
    fn tab_url_substring_match() {
        let ts = tabs();
        let result = resolve_tab(&ts, Some("crates"), None).unwrap();
        assert_eq!(result.actor.as_ref(), "server1.conn0.tab2");
    }

    #[test]
    fn tab_url_substring_case_insensitive() {
        let ts = tabs();
        let result = resolve_tab(&ts, Some("GitHub"), None).unwrap();
        assert_eq!(result.actor.as_ref(), "server1.conn0.tab1");
    }

    #[test]
    fn tab_url_substring_no_match_returns_error() {
        let ts = tabs();
        let err = resolve_tab(&ts, Some("wikipedia"), None).unwrap_err();
        match err {
            AppError::User(msg) => assert!(msg.contains("wikipedia"), "message: {msg}"),
            other => panic!("expected AppError::User, got: {other}"),
        }
    }

    // --- default (no flags) ---

    #[test]
    fn default_returns_selected_tab() {
        let ts = tabs();
        let result = resolve_tab(&ts, None, None).unwrap();
        // tab2 is selected
        assert_eq!(result.actor.as_ref(), "server1.conn0.tab2");
    }

    #[test]
    fn default_falls_back_to_first_when_none_selected() {
        let ts = vec![
            make_tab("server1.conn0.tab1", "https://example.com", false),
            make_tab("server1.conn0.tab2", "https://rust-lang.org", false),
        ];
        let result = resolve_tab(&ts, None, None).unwrap();
        assert_eq!(result.actor.as_ref(), "server1.conn0.tab1");
    }

    #[test]
    fn default_empty_tabs_returns_error() {
        let err = resolve_tab(&[], None, None).unwrap_err();
        match err {
            AppError::User(msg) => assert!(msg.contains("Firefox"), "message: {msg}"),
            other => panic!("expected AppError::User, got: {other}"),
        }
    }

    // --- tab_id takes precedence over tab ---

    #[test]
    fn tab_id_takes_precedence_over_tab() {
        let ts = tabs();
        // --tab 1 would give tab1, but --tab-id for tab3 should win
        let result = resolve_tab(&ts, Some("1"), Some("server1.conn0.tab3")).unwrap();
        assert_eq!(result.actor.as_ref(), "server1.conn0.tab3");
    }
}
