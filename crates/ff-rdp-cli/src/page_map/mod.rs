//! Page-map format: types, deserialization, and resolver methods.
//!
//! The canonical format is JSON (`draft-2020-12` schema at
//! `schemas/page-map.schema.json`), but YAML is accepted as input.
//! Both parse to the same in-memory representation.
//!
//! A page-map is a pre-computed index of a site that an agent reads
//! once per session instead of spending multiple turns discovering
//! "what's on this page."

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use anyhow::{Context as _, bail};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

pub const PAGE_MAP_SCHEMA_URL: &str = "https://ff-rdp.dev/schemas/page-map/v1.json";
pub const PAGE_MAP_VERSION: u32 = 1;

// ---------------------------------------------------------------------------
// Top-level PageMap
// ---------------------------------------------------------------------------

/// A pre-computed site index used by the script runner to resolve
/// dotted-path references to CSS selectors and API routes.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PageMap {
    /// JSON Schema discriminator — must be [`PAGE_MAP_SCHEMA_URL`] when present.
    #[serde(rename = "$schema", skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,

    /// Format version — must be [`PAGE_MAP_VERSION`].
    pub version: u32,

    /// Timestamp when the map was generated.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generated_at: Option<DateTime<Utc>>,

    /// Base URL the map was crawled from.
    pub base_url: String,

    /// Pages keyed by URL path (e.g. `"sign-in"`, `"users"`).
    #[serde(default)]
    pub pages: BTreeMap<String, Page>,

    /// Named API routes (method + path) for `assert_network: { api_route: <name> }`.
    #[serde(default)]
    pub api_routes: BTreeMap<String, ApiRoute>,

    /// Named reusable script bodies (iter-61 script minus top-level metadata).
    #[serde(default)]
    pub flows: BTreeMap<String, FlowBody>,
}

impl PageMap {
    /// Load a page-map from a file, accepting JSON or YAML.
    pub fn load(path: &Path) -> anyhow::Result<Arc<Self>> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("reading page-map file '{}'", path.display()))?;
        let fmt = PageMapFormat::from_path(path);
        let map = parse_page_map_str(&content, fmt)
            .with_context(|| format!("parsing page-map '{}'", path.display()))?;
        Ok(Arc::new(map))
    }

    /// Try to load a page-map from the default location (`.ffrdp/page-map.json`),
    /// returning `None` if the file does not exist.
    pub fn load_default() -> anyhow::Result<Option<Arc<Self>>> {
        let default_path = Path::new(".ffrdp").join("page-map.json");
        if !default_path.exists() {
            return Ok(None);
        }
        Self::load(&default_path).map(Some)
    }

    /// Resolve a dotted-path reference to a CSS selector.
    ///
    /// Supported path forms:
    /// - `pages.<page>.forms.<form>.submit` → submit button selector
    /// - `pages.<page>.forms.<form>.fields.<name>` → field selector
    /// - `pages.<page>.forms.<form>` → form selector
    ///
    /// Returns an error when the path does not resolve.
    pub fn resolve_target(&self, dotted_path: &str) -> anyhow::Result<String> {
        let parts: Vec<&str> = dotted_path.splitn(8, '.').collect();

        match parts.as_slice() {
            // pages.<page>.forms.<form>.submit
            ["pages", page, "forms", form, "submit"] => {
                let p = self.pages.get(*page).with_context(|| {
                    format!("page-map: page '{page}' not found in pages (path: {dotted_path})")
                })?;
                let f = p.forms.iter().find(|f| f.id.as_deref() == Some(form)).with_context(|| {
                    format!(
                        "page-map: form '{form}' not found in pages.{page}.forms (path: {dotted_path})"
                    )
                })?;
                Ok(f.submit.selector.clone())
            }

            // pages.<page>.forms.<form>.fields.<field>
            ["pages", page, "forms", form, "fields", field] => {
                let p = self.pages.get(*page).with_context(|| {
                    format!("page-map: page '{page}' not found in pages (path: {dotted_path})")
                })?;
                let f = p.forms.iter().find(|f| f.id.as_deref() == Some(form)).with_context(|| {
                    format!(
                        "page-map: form '{form}' not found in pages.{page}.forms (path: {dotted_path})"
                    )
                })?;
                let field_entry =
                    f.fields.iter().find(|ff| ff.name == *field).with_context(|| {
                        format!(
                            "page-map: field '{field}' not found in pages.{page}.forms.{form}.fields (path: {dotted_path})"
                        )
                    })?;
                Ok(field_entry.selector.clone())
            }

            // pages.<page>.forms.<form>
            ["pages", page, "forms", form] => {
                let p = self.pages.get(*page).with_context(|| {
                    format!("page-map: page '{page}' not found in pages (path: {dotted_path})")
                })?;
                let f = p.forms.iter().find(|f| f.id.as_deref() == Some(form)).with_context(|| {
                    format!(
                        "page-map: form '{form}' not found in pages.{page}.forms (path: {dotted_path})"
                    )
                })?;
                Ok(f.selector.clone())
            }

            _ => bail!(
                "page-map: unrecognised path '{dotted_path}' — supported: \
                 pages.<page>.forms.<form>, \
                 pages.<page>.forms.<form>.submit, \
                 pages.<page>.forms.<form>.fields.<name>"
            ),
        }
    }

    /// Resolve a named API route to `(method, path)`.
    pub fn resolve_api_route(&self, name: &str) -> anyhow::Result<(&str, &str)> {
        let route = self.api_routes.get(name).with_context(|| {
            format!(
                "page-map: api_route '{name}' not found — available: [{}]",
                self.api_routes
                    .keys()
                    .map(String::as_str)
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })?;
        Ok((&route.method, &route.path))
    }

    /// Resolve a named flow body.
    // allow-claim-miss: resolve_flow — first_call_site is the `run: {flow: <name>}` step
    // which is planned for the D-flows task; wired up here as the resolver, called from there.
    #[allow(dead_code)]
    pub fn resolve_flow(&self, name: &str) -> anyhow::Result<&FlowBody> {
        self.flows.get(name).with_context(|| {
            format!(
                "page-map: flow '{name}' not found — available: [{}]",
                self.flows
                    .keys()
                    .map(String::as_str)
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })
    }
}

// ---------------------------------------------------------------------------
// Page
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Page {
    /// URL path for this page (e.g. `/sign-in`).
    pub path: String,

    /// Document `<title>` or inferred heading.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    /// Whether this page requires authentication (detected via redirect heuristic).
    #[serde(default)]
    pub auth_required: bool,

    /// Named ARIA landmarks.
    #[serde(default)]
    pub landmarks: Vec<Landmark>,

    /// Forms found on the page.
    #[serde(default)]
    pub forms: Vec<Form>,

    /// Curated outgoing links (not every `<a>`, just important routes).
    #[serde(default)]
    pub links: Vec<Link>,
}

// ---------------------------------------------------------------------------
// Landmark
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Landmark {
    /// ARIA role name (e.g. `"navigation"`, `"main"`).
    pub name: String,

    /// CSS selector for the landmark element.
    pub region: String,

    /// Top interactive elements within the landmark.
    #[serde(default)]
    pub elements: Vec<LandmarkElement>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LandmarkElement {
    /// ARIA label or visible text.
    pub label: String,

    /// CSS selector.
    pub selector: String,

    /// Element role (e.g. `"link"`, `"button"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
}

// ---------------------------------------------------------------------------
// Form
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Form {
    /// Stable identifier used in dotted-path references
    /// (e.g. `pages.sign-in.forms.signin`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// CSS selector for the form element (or its closest container).
    pub selector: String,

    /// Input fields.
    #[serde(default)]
    pub fields: Vec<Field>,

    /// Submit button.
    pub submit: Submit,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Field {
    /// `name` attribute (or inferred name).
    pub name: String,

    /// CSS selector for the input element.
    pub selector: String,

    /// HTML input type (e.g. `"email"`, `"password"`, `"text"`).
    #[serde(rename = "type")]
    pub type_: String,

    /// Whether the field is required.
    #[serde(default)]
    pub required: bool,

    /// Placeholder text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub placeholder: Option<String>,

    /// Validation hint (pattern, min/max, etc.).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub validation: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Submit {
    /// CSS selector for the submit button.
    pub selector: String,

    /// URL the form posts to (from `action` attribute or inferred).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub posts_to: Option<String>,

    /// HTTP method (default `"POST"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
}

// ---------------------------------------------------------------------------
// Link
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Link {
    /// Link text or ARIA label.
    pub label: String,

    /// Destination URL path.
    pub href: String,

    /// CSS selector.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selector: Option<String>,
}

// ---------------------------------------------------------------------------
// ApiRoute
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ApiRoute {
    /// Human-readable name (redundant with the map key, kept for
    /// round-trip fidelity).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// HTTP method (e.g. `"POST"`, `"GET"`).
    pub method: String,

    /// URL path (e.g. `"/api/auth/sign-in/email"`).
    pub path: String,

    /// Optional request shape description (opaque, for documentation).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request: Option<Value>,

    /// Optional response shape description (opaque, for documentation).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response: Option<Value>,
}

// ---------------------------------------------------------------------------
// FlowBody — iter-61 script body minus top-level metadata
// ---------------------------------------------------------------------------

/// A reusable script body stored inside a page-map.
///
/// Equivalent to an iter-61 `Script` minus the `$schema`, `version`, `name`,
/// `base_url`, and `page_map` fields — those are inherited from the
/// containing page-map.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FlowBody {
    /// Steps to execute, in order.
    pub steps: Vec<Value>,

    /// Variable defaults (overridable by the caller).
    #[serde(default, skip_serializing_if = "std::collections::HashMap::is_empty")]
    pub vars: std::collections::HashMap<String, String>,

    /// Opaque metadata (not interpreted by the runner).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
}

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

/// Canonical serialization format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageMapFormat {
    Json,
    Yaml,
}

impl PageMapFormat {
    pub fn from_path(path: &Path) -> Self {
        match path.extension().and_then(|e| e.to_str()) {
            Some("yaml" | "yml") => Self::Yaml,
            _ => Self::Json,
        }
    }
}

/// Parse a page-map from raw text.
///
/// YAML input is converted to `serde_json::Value` first so that both
/// formats use identical in-memory types.
pub fn parse_page_map_str(content: &str, fmt: PageMapFormat) -> anyhow::Result<PageMap> {
    let map: PageMap = match fmt {
        PageMapFormat::Json => serde_json::from_str(content).context("JSON parse error")?,
        PageMapFormat::Yaml => {
            let value: serde_json::Value =
                serde_norway::from_str(content).context("YAML parse error")?;
            serde_json::from_value(value).context("YAML→JSON conversion")?
        }
    };
    validate_page_map(&map)?;
    Ok(map)
}

/// Parse a page-map from a file.
pub fn parse_page_map_file(path: &Path) -> anyhow::Result<PageMap> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("reading page-map '{}'", path.display()))?;
    let fmt = PageMapFormat::from_path(path);
    parse_page_map_str(&content, fmt)
        .with_context(|| format!("parsing page-map '{}'", path.display()))
}

/// Validate parser-level invariants.
pub fn validate_page_map(map: &PageMap) -> anyhow::Result<()> {
    if map.version != PAGE_MAP_VERSION {
        bail!(
            "page-map version {} is not supported (expected {}); \
             please regenerate the map with `ff-rdp index`",
            map.version,
            PAGE_MAP_VERSION
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Drift detection
// ---------------------------------------------------------------------------

/// A single detected drift between an existing and a re-crawled page-map.
#[derive(Debug, Clone, Serialize)]
pub struct DriftEntry {
    /// Dotted path where the drift was detected.
    pub path: String,

    /// Kind of drift: `"selector_changed"`, `"missing"`, `"added"`.
    pub kind: String,

    /// Old value (when applicable).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub old: Option<String>,

    /// New value (when applicable).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new: Option<String>,
}

/// Compare two page-maps and return a list of drifted entries.
pub fn detect_drift(old: &PageMap, new: &PageMap) -> Vec<DriftEntry> {
    let mut drifts: Vec<DriftEntry> = Vec::new();

    // Check pages present in old map.
    for (page_key, old_page) in &old.pages {
        let path_prefix = format!("pages.{page_key}");
        match new.pages.get(page_key) {
            None => {
                drifts.push(DriftEntry {
                    path: path_prefix.clone(),
                    kind: "missing".to_owned(),
                    old: Some(old_page.path.clone()),
                    new: None,
                });
            }
            Some(new_page) => {
                // Compare forms.
                for old_form in &old_page.forms {
                    let form_id = old_form.id.as_deref().unwrap_or("<unnamed>");
                    let form_prefix = format!("{path_prefix}.forms.{form_id}");

                    // Form selector drift.
                    if let Some(new_form) = new_page.forms.iter().find(|f| f.id == old_form.id) {
                        if old_form.selector != new_form.selector {
                            drifts.push(DriftEntry {
                                path: form_prefix.clone(),
                                kind: "selector_changed".to_owned(),
                                old: Some(old_form.selector.clone()),
                                new: Some(new_form.selector.clone()),
                            });
                        }
                        // Submit selector drift.
                        if old_form.submit.selector != new_form.submit.selector {
                            drifts.push(DriftEntry {
                                path: format!("{form_prefix}.submit"),
                                kind: "selector_changed".to_owned(),
                                old: Some(old_form.submit.selector.clone()),
                                new: Some(new_form.submit.selector.clone()),
                            });
                        }
                        // Field selector drifts.
                        for old_field in &old_form.fields {
                            let field_prefix = format!("{form_prefix}.fields.{}", old_field.name);
                            match new_form.fields.iter().find(|f| f.name == old_field.name) {
                                None => {
                                    drifts.push(DriftEntry {
                                        path: field_prefix,
                                        kind: "missing".to_owned(),
                                        old: Some(old_field.selector.clone()),
                                        new: None,
                                    });
                                }
                                Some(new_field) => {
                                    if old_field.selector != new_field.selector {
                                        drifts.push(DriftEntry {
                                            path: field_prefix,
                                            kind: "selector_changed".to_owned(),
                                            old: Some(old_field.selector.clone()),
                                            new: Some(new_field.selector.clone()),
                                        });
                                    }
                                }
                            }
                        }
                    } else {
                        drifts.push(DriftEntry {
                            path: form_prefix,
                            kind: "missing".to_owned(),
                            old: Some(old_form.selector.clone()),
                            new: None,
                        });
                    }
                }
            }
        }
    }

    // Pages present in new map but not old.
    for page_key in new.pages.keys() {
        if !old.pages.contains_key(page_key) {
            let new_page = &new.pages[page_key];
            drifts.push(DriftEntry {
                path: format!("pages.{page_key}"),
                kind: "added".to_owned(),
                old: None,
                new: Some(new_page.path.clone()),
            });
        }
    }

    // API route drifts.
    for (route_name, old_route) in &old.api_routes {
        match new.api_routes.get(route_name) {
            None => {
                drifts.push(DriftEntry {
                    path: format!("api_routes.{route_name}"),
                    kind: "missing".to_owned(),
                    old: Some(format!("{} {}", old_route.method, old_route.path)),
                    new: None,
                });
            }
            Some(new_route) => {
                if old_route.method != new_route.method || old_route.path != new_route.path {
                    drifts.push(DriftEntry {
                        path: format!("api_routes.{route_name}"),
                        kind: "selector_changed".to_owned(),
                        old: Some(format!("{} {}", old_route.method, old_route.path)),
                        new: Some(format!("{} {}", new_route.method, new_route.path)),
                    });
                }
            }
        }
    }

    drifts
}

// ---------------------------------------------------------------------------
// JS template for form extraction
// ---------------------------------------------------------------------------

/// JavaScript template injected into each crawled page to extract form data.
pub fn form_extraction_js_template() -> &'static str {
    r#"(function extractForms() {
  const forms = [];
  // Collect named <form> elements.
  document.querySelectorAll('form').forEach(function(form, fi) {
    const fields = [];
    form.querySelectorAll('input, select, textarea').forEach(function(el) {
      if (el.type === 'hidden' || el.type === 'submit' || el.type === 'button') return;
      const name = el.name || el.id || el.getAttribute('aria-label') || ('field_' + fields.length);
      fields.push({
        name: name,
        type: el.type || 'text',
        required: el.required || el.getAttribute('aria-required') === 'true',
        placeholder: el.placeholder || null,
        selector: el.id ? '#' + el.id : (el.name ? '[name="' + el.name + '"]' : null)
      });
    });
    const submitEl = form.querySelector('[type="submit"], button:not([type])');
    const formId = form.id || form.getAttribute('data-form-id') || ('form_' + fi);
    forms.push({
      id: formId,
      selector: form.id ? ('form#' + form.id) : ('form:nth-of-type(' + (fi + 1) + ')'),
      action: form.action || null,
      method: (form.method || 'post').toUpperCase(),
      fields: fields,
      submit: {
        selector: submitEl ? (submitEl.id ? '#' + submitEl.id : (submitEl.type === 'submit' ? '[type="submit"]' : 'button:not([type])')) : '[type="submit"]',
        posts_to: form.action || null
      }
    });
  });
  return JSON.stringify(forms);
})()"#
}

/// JavaScript template for extracting ARIA landmarks.
pub fn landmark_extraction_js_template() -> &'static str {
    r#"(function extractLandmarks() {
  const roleMap = {
    'nav': 'navigation', 'main': 'main', 'aside': 'complementary',
    'header': 'banner', 'footer': 'contentinfo', 'form': 'form', 'search': 'search'
  };
  const landmarks = [];
  const seen = new Set();
  function addLandmark(el, role) {
    if (seen.has(el)) return;
    seen.add(el);
    const sel = el.id ? ('[role="' + role + '"]#' + el.id) : ('[role="' + role + '"]');
    const elements = [];
    el.querySelectorAll('a[href], button, [role="button"], [role="link"]').forEach(function(child, i) {
      if (i >= 10) return;
      const label = child.getAttribute('aria-label') || child.textContent.trim().slice(0, 60);
      const childSel = child.id ? '#' + child.id : child.tagName.toLowerCase();
      elements.push({ label: label, selector: childSel, role: child.getAttribute('role') || child.tagName.toLowerCase() });
    });
    landmarks.push({ name: role, region: el.id ? ('#' + el.id) : el.tagName.toLowerCase(), elements: elements });
  }
  document.querySelectorAll('[role]').forEach(function(el) {
    const role = el.getAttribute('role');
    if (['navigation','main','complementary','banner','contentinfo','search'].includes(role)) addLandmark(el, role);
  });
  Object.entries(roleMap).forEach(function([tag, role]) {
    document.querySelectorAll(tag).forEach(function(el) { addLandmark(el, role); });
  });
  return JSON.stringify(landmarks);
})()"#
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Helper: minimal valid page-map JSON.
    // -----------------------------------------------------------------------
    fn minimal_json() -> String {
        // Build the JSON string at runtime to avoid raw-string terminator issues
        // with CSS selectors like #submit-btn that confuse the `r#"..."#` scanner.
        serde_json::json!({
            "version": 1,
            "base_url": "https://example.com",
            "pages": {
                "login": {
                    "path": "/login",
                    "title": "Log In",
                    "forms": [{
                        "id": "signin",
                        "selector": "form#signin",
                        "fields": [
                            {"name": "email", "selector": "#email", "type": "email", "required": true},
                            {"name": "password", "selector": "#password", "type": "password", "required": true}
                        ],
                        "submit": {"selector": "#submit-btn"}
                    }]
                }
            },
            "api_routes": {
                "sign-in": {"method": "POST", "path": "/api/auth/sign-in"}
            }
        })
        .to_string()
    }

    fn minimal_yaml() -> &'static str {
        "version: 1\nbase_url: https://example.com\n"
    }

    fn parse_minimal() -> PageMap {
        parse_page_map_str(&minimal_json(), PageMapFormat::Json).expect("parse minimal_json")
    }

    // -----------------------------------------------------------------------
    // test_handwritten_page_map_validates — JSON and YAML
    // -----------------------------------------------------------------------

    #[test]
    fn test_handwritten_page_map_validates_json() {
        let map = parse_page_map_str(&minimal_json(), PageMapFormat::Json);
        assert!(map.is_ok(), "minimal JSON should parse: {map:?}");
        let map = map.unwrap();
        assert_eq!(map.version, PAGE_MAP_VERSION);
        assert_eq!(map.base_url, "https://example.com");
    }

    #[test]
    fn test_handwritten_page_map_validates_yaml() {
        let map = parse_page_map_str(minimal_yaml(), PageMapFormat::Yaml);
        assert!(map.is_ok(), "minimal YAML should parse: {map:?}");
        let map = map.unwrap();
        assert_eq!(map.version, PAGE_MAP_VERSION);
    }

    #[test]
    fn test_deny_unknown_fields_rejects_stray_key() {
        let json = r#"{"version": 1, "base_url": "https://x.com", "unknown_key": true}"#;
        let result = parse_page_map_str(json, PageMapFormat::Json);
        assert!(
            result.is_err(),
            "deny_unknown_fields: stray key should fail"
        );
    }

    // -----------------------------------------------------------------------
    // test_page_map_version_check_rejects_v2
    // -----------------------------------------------------------------------

    #[test]
    fn test_page_map_version_check_rejects_v2() {
        let json = r#"{"version": 2, "base_url": "https://x.com"}"#;
        let result = parse_page_map_str(json, PageMapFormat::Json);
        assert!(result.is_err(), "version 2 should be rejected");
        let msg = format!("{}", result.unwrap_err());
        assert!(
            msg.contains("version") || msg.contains("supported"),
            "error should mention version: {msg}"
        );
    }

    // -----------------------------------------------------------------------
    // test_page_map_resolve_target
    // -----------------------------------------------------------------------

    #[test]
    fn test_resolve_target_form_selector() {
        let map = parse_minimal();
        let sel = map
            .resolve_target("pages.login.forms.signin")
            .expect("form selector");
        assert_eq!(sel, "form#signin");
    }

    #[test]
    fn test_resolve_target_submit_selector() {
        let map = parse_minimal();
        let sel = map
            .resolve_target("pages.login.forms.signin.submit")
            .expect("submit selector");
        assert_eq!(sel, "#submit-btn");
    }

    #[test]
    fn test_resolve_target_field_selector_email() {
        let map = parse_minimal();
        let sel = map
            .resolve_target("pages.login.forms.signin.fields.email")
            .expect("email selector");
        assert_eq!(sel, "#email");
    }

    #[test]
    fn test_resolve_target_field_selector_password() {
        let map = parse_minimal();
        let sel = map
            .resolve_target("pages.login.forms.signin.fields.password")
            .expect("password selector");
        assert_eq!(sel, "#password");
    }

    #[test]
    fn test_resolve_target_unknown_page_errors() {
        let map = parse_minimal();
        let result = map.resolve_target("pages.nonexistent.forms.signin");
        assert!(result.is_err(), "unknown page should error");
        let msg = format!("{}", result.unwrap_err());
        assert!(
            msg.contains("nonexistent"),
            "error should name the missing page: {msg}"
        );
    }

    #[test]
    fn test_resolve_target_unknown_form_errors() {
        let map = parse_minimal();
        let result = map.resolve_target("pages.login.forms.badform");
        assert!(result.is_err(), "unknown form should error");
        let msg = format!("{}", result.unwrap_err());
        assert!(
            msg.contains("badform"),
            "error should name the missing form: {msg}"
        );
    }

    #[test]
    fn test_resolve_target_unknown_field_errors() {
        let map = parse_minimal();
        let result = map.resolve_target("pages.login.forms.signin.fields.nofield");
        assert!(result.is_err(), "unknown field should error");
        let msg = format!("{}", result.unwrap_err());
        assert!(
            msg.contains("nofield"),
            "error should name the missing field: {msg}"
        );
    }

    #[test]
    fn test_resolve_target_unrecognised_path_errors() {
        let map = parse_minimal();
        let result = map.resolve_target("pages.login");
        assert!(result.is_err(), "incomplete path should error");
    }

    // -----------------------------------------------------------------------
    // test_page_map_resolve_api_route
    // -----------------------------------------------------------------------

    #[test]
    fn test_resolve_api_route_found() {
        let map = parse_minimal();
        let (method, path) = map.resolve_api_route("sign-in").expect("route found");
        assert_eq!(method, "POST");
        assert_eq!(path, "/api/auth/sign-in");
    }

    #[test]
    fn test_resolve_api_route_missing_errors() {
        let map = parse_minimal();
        let result = map.resolve_api_route("not-a-route");
        assert!(result.is_err(), "missing api route should error");
        let msg = format!("{}", result.unwrap_err());
        assert!(
            msg.contains("not-a-route"),
            "error should name the missing route: {msg}"
        );
    }

    // -----------------------------------------------------------------------
    // test_form_extraction_js_template_shape
    // -----------------------------------------------------------------------

    #[test]
    fn test_form_extraction_js_template_shape() {
        let js = form_extraction_js_template();
        assert!(js.contains("extractForms"), "should define extractForms");
        assert!(js.contains("querySelectorAll"), "should query DOM");
        assert!(js.contains("JSON.stringify"), "should return JSON string");
    }

    #[test]
    fn test_landmark_extraction_js_template_shape() {
        let js = landmark_extraction_js_template();
        assert!(
            js.contains("extractLandmarks"),
            "should define extractLandmarks"
        );
        assert!(js.contains("JSON.stringify"), "should return JSON string");
    }

    // -----------------------------------------------------------------------
    // test_drift_detection_finds_mutated_selector
    // -----------------------------------------------------------------------

    #[test]
    fn test_drift_detection_finds_mutated_selector() {
        let old_json = minimal_json();
        let new_json = old_json.replace("#submit-btn", "#new-submit");

        let old = parse_page_map_str(&old_json, PageMapFormat::Json).unwrap();
        let new = parse_page_map_str(&new_json, PageMapFormat::Json).unwrap();

        let drifts = detect_drift(&old, &new);
        assert!(!drifts.is_empty(), "should detect submit selector drift");
        let submit_drift = drifts
            .iter()
            .find(|d| d.path.contains("submit"))
            .expect("submit drift entry");
        assert_eq!(submit_drift.kind, "selector_changed");
        assert_eq!(submit_drift.old.as_deref(), Some("#submit-btn"));
        assert_eq!(submit_drift.new.as_deref(), Some("#new-submit"));
    }

    #[test]
    fn test_drift_detection_empty_when_maps_equal() {
        let map = parse_page_map_str(&minimal_json(), PageMapFormat::Json).unwrap();
        let drifts = detect_drift(&map, &map.clone());
        assert!(drifts.is_empty(), "no drift when maps are identical");
    }

    fn make_page_map_with_extra_page(extra_key: &str) -> PageMap {
        let json = serde_json::json!({
            "version": 1,
            "base_url": "https://example.com",
            "pages": {
                "login": {
                    "path": "/login",
                    "forms": [{
                        "id": "signin",
                        "selector": "form#signin",
                        "fields": [
                            {"name": "email", "selector": "#email", "type": "email"},
                            {"name": "password", "selector": "#password", "type": "password"}
                        ],
                        "submit": {"selector": "#submit-btn"}
                    }]
                },
                extra_key: {
                    "path": format!("/{extra_key}")
                }
            }
        })
        .to_string();
        parse_page_map_str(&json, PageMapFormat::Json).expect("map with extra page")
    }

    #[test]
    fn test_drift_detection_added_page() {
        let old = parse_minimal();
        let new = make_page_map_with_extra_page("dashboard");
        let drifts = detect_drift(&old, &new);
        let added = drifts
            .iter()
            .find(|d| d.kind == "added" && d.path.contains("dashboard"));
        assert!(added.is_some(), "should detect added dashboard page");
    }

    #[test]
    fn test_drift_detection_missing_page() {
        let old = make_page_map_with_extra_page("extra");
        let new = parse_minimal();
        let drifts = detect_drift(&old, &new);
        let missing = drifts
            .iter()
            .find(|d| d.kind == "missing" && d.path.contains("extra"));
        assert!(missing.is_some(), "should detect missing extra page");
    }

    // -----------------------------------------------------------------------
    // test_page_map_flow_resolves_to_script (basic wiring only)
    // -----------------------------------------------------------------------

    #[test]
    fn test_page_map_flow_resolves() {
        let json = r#"{
            "version": 1,
            "base_url": "https://x.com",
            "flows": {
                "login-flow": {
                    "steps": [{"navigate": {"url": "/login"}}]
                }
            }
        }"#;
        let map = parse_page_map_str(json, PageMapFormat::Json).expect("parse with flows");
        let flow = map.resolve_flow("login-flow").expect("flow found");
        assert_eq!(flow.steps.len(), 1);
    }

    #[test]
    fn test_page_map_flow_missing_errors() {
        let map = parse_page_map_str(&minimal_json(), PageMapFormat::Json).unwrap();
        let result = map.resolve_flow("nonexistent");
        assert!(result.is_err(), "missing flow should error");
    }
}
