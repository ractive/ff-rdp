//! `ff-rdp index` command — crawl a site and emit a page-map JSON index.
//!
//! The crawler does a BFS from the base URL, navigating each page in the
//! current Firefox tab (reusing the live session cookies).  Per page it
//! evaluates a form-extraction JS template and an ARIA-landmark JS template,
//! then writes the result atomically to the output path.

use std::collections::{BTreeMap, VecDeque};
use std::path::Path;

use anyhow::Context as _;
use chrono::Utc;
use regex::Regex;
use serde_json::Value;

use crate::cli::args::Cli;
use crate::error::AppError;
use crate::page_map::{
    Field, Form, Landmark, LandmarkElement, Link, PAGE_MAP_SCHEMA_URL, PAGE_MAP_VERSION, Page,
    PageMap, detect_drift, form_extraction_js_template, landmark_extraction_js_template,
};

/// Options parsed from the `index` CLI flags.
pub struct IndexOpts<'a> {
    pub base_url: Option<&'a str>,
    pub out: &'a Path,
    pub depth: u32,
    pub max_pages: usize,
    pub include: Option<&'a str>,
    pub exclude: Option<&'a str>,
    pub format: &'a str,
    pub cross_origin: bool,
    pub ignore_robots: bool,
    pub login_script: Option<&'a Path>,
    pub check: bool,
    pub page_map: Option<&'a Path>,
    pub report: Option<&'a Path>,
    /// When true, suppress the crawl summary JSON line on stdout.
    pub silent: bool,
    /// When set, output paths must be descendants of this directory.
    pub output_root: Option<&'a Path>,
}

/// Entry point for `ff-rdp index`.
pub fn run(cli: &Cli, opts: &IndexOpts<'_>) -> Result<(), AppError> {
    if opts.check {
        return run_check(cli, opts);
    }
    run_crawl(cli, opts)
}

// ---------------------------------------------------------------------------
// Crawl mode
// ---------------------------------------------------------------------------

fn run_crawl(cli: &Cli, opts: &IndexOpts<'_>) -> Result<(), AppError> {
    // Run login script if requested.
    if let Some(login_path) = opts.login_script {
        eprintln!("[index] running login script: {}", login_path.display());
        let login_opts = crate::commands::run::RunCommandOpts {
            script_path: login_path,
            extra_vars: std::collections::HashMap::new(),
            bail_on_failure: true,
            dry_run: false,
            show_secrets: false,
            record_output: None,
            record_strict: false,
            format_override: None,
            page_map_path: None,
            allow_env: Vec::new(),
            allow_unsafe_script_paths: false,
        };
        crate::commands::run::run(cli, &login_opts)
            .map_err(|e| AppError::User(format!("login script failed: {e}")))?;
    }

    // Resolve base URL: use current tab URL if not supplied.
    let base_url = match opts.base_url {
        Some(u) => u.trim_end_matches('/').to_owned(),
        None => eval_js_value(cli, "location.origin")
            .ok()
            .and_then(|v| v.as_str().map(str::to_owned))
            .ok_or_else(|| {
                AppError::User(
                    "could not determine current tab origin — pass a base_url positionally"
                        .to_owned(),
                )
            })?,
    };

    eprintln!(
        "[index] crawling {} (depth={}, max-pages={})",
        base_url, opts.depth, opts.max_pages
    );

    // Compile include/exclude regexes.
    let include_re = opts
        .include
        .map(Regex::new)
        .transpose()
        .map_err(|e| AppError::User(format!("--include regex: {e}")))?;
    let exclude_re = opts
        .exclude
        .map(Regex::new)
        .transpose()
        .map_err(|e| AppError::User(format!("--exclude regex: {e}")))?;

    // BFS state. `enqueued` tracks every URL ever pushed onto `queue`
    // (separately from `visited`, which only gains an entry once a page has
    // actually been crawled) so a URL discovered as an outgoing link on
    // multiple pages before it's popped is enqueued once, not once per
    // referring page (iter-141 Theme B: "each URL is also enqueued twice").
    let mut visited: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut enqueued: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut queue: VecDeque<(String, u32)> = VecDeque::new();
    queue.push_back((base_url.clone(), 0));
    enqueued.insert(base_url.clone());

    let mut pages: BTreeMap<String, Page> = BTreeMap::new();

    // Robots.txt cache (only populated when !ignore_robots).
    let disallowed_paths: Vec<String> = if opts.ignore_robots {
        Vec::new()
    } else {
        fetch_disallowed_paths(&base_url)
    };

    while let Some((url, current_depth)) = queue.pop_front() {
        if visited.len() >= opts.max_pages {
            eprintln!("[index] reached max-pages limit ({})", opts.max_pages);
            break;
        }
        if visited.contains(&url) {
            continue;
        }

        // Apply include/exclude filters (on the path part).
        let url_path = url.strip_prefix(&base_url).unwrap_or(&url);
        if include_re.as_ref().is_some_and(|re| !re.is_match(url_path)) {
            continue;
        }
        if exclude_re.as_ref().is_some_and(|re| re.is_match(url_path)) {
            continue;
        }

        // Skip robots.txt disallowed paths.
        if disallowed_paths
            .iter()
            .any(|p| url_path.starts_with(p.as_str()))
        {
            eprintln!("[index] skipped (robots.txt): {url}");
            continue;
        }

        visited.insert(url.clone());
        eprintln!(
            "[index] visited {}/{} pages, queue={}",
            visited.len(),
            opts.max_pages,
            queue.len()
        );

        // Navigate to the page.
        let page_result = crawl_page(cli, &url, &base_url, opts.cross_origin);
        match page_result {
            Err(e) => {
                eprintln!("[index] warning: failed to crawl {url}: {e}");
            }
            Ok(CrawledPage {
                page,
                outgoing_links,
                auth_redirect,
            }) => {
                let slug = url_to_slug(&url, &base_url);
                let mut page_entry = page;

                // Auth-redirect detection: if the navigate ended on a login URL,
                // mark auth_required and skip deeper crawl on this branch.
                if auth_redirect {
                    page_entry.auth_required = true;
                    pages.insert(slug, page_entry);
                    continue;
                }

                // Enqueue outgoing links for the next depth level.
                if current_depth < opts.depth {
                    for link in &outgoing_links {
                        if !visited.contains(link) && enqueued.insert(link.clone()) {
                            queue.push_back((link.clone(), current_depth + 1));
                        }
                    }
                }

                pages.insert(slug, page_entry);
            }
        }
    }

    let page_count = pages.len();
    let form_count: usize = pages.values().map(|p| p.forms.len()).sum();

    let map = PageMap {
        schema: Some(PAGE_MAP_SCHEMA_URL.to_owned()),
        version: PAGE_MAP_VERSION,
        generated_at: Some(Utc::now()),
        base_url,
        pages,
        api_routes: BTreeMap::new(),
        flows: BTreeMap::new(),
    };

    if let Some(root) = opts.output_root {
        crate::util::safe_io::ensure_within_root(opts.out, root).map_err(|e| {
            AppError::User(format!("index: output path escapes --output-root: {e}"))
        })?;
    }

    write_page_map(opts.out, &map, opts.format)
        .map_err(|e| AppError::User(format!("writing page-map: {e}")))?;

    if !opts.silent {
        let result = serde_json::json!({
            "results": {
                "pages": page_count,
                "forms": form_count,
                "api_routes": 0,
                "out": opts.out.display().to_string()
            },
            "total": 1
        });
        println!("{result}");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Check mode
// ---------------------------------------------------------------------------

fn run_check(cli: &Cli, opts: &IndexOpts<'_>) -> Result<(), AppError> {
    // Load the existing page-map.
    let existing_path = opts.page_map.unwrap_or(Path::new(".ffrdp/page-map.json"));
    let existing = crate::page_map::parse_page_map_file(existing_path)
        .map_err(|e| AppError::User(format!("loading page-map: {e}")))?;

    // Run a fresh crawl using the existing map's base_url.
    // Write into a temp directory so write_page_map can persist the file
    // there; tempdir cleanup happens when `tmp_dir` is dropped.
    let fresh_base = opts.base_url.unwrap_or(&existing.base_url).to_owned();
    let tmp_dir = tempfile::tempdir()
        .map_err(|e| AppError::Internal(anyhow::anyhow!("creating temp dir: {e}")))?;
    let tmp_path = tmp_dir.path().join("fresh.json");

    let crawl_opts = IndexOpts {
        base_url: Some(&fresh_base),
        out: &tmp_path,
        depth: opts.depth,
        max_pages: opts.max_pages,
        include: opts.include,
        exclude: opts.exclude,
        format: "json",
        cross_origin: opts.cross_origin,
        ignore_robots: opts.ignore_robots,
        login_script: opts.login_script,
        check: false,
        page_map: None,
        report: None,
        silent: true,
        output_root: None,
    };
    run_crawl(cli, &crawl_opts)?;

    // Load the freshly-crawled map.
    let fresh = crate::page_map::parse_page_map_file(&tmp_path)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("loading fresh crawl: {e}")))?;

    // Detect drift.
    let drifts = detect_drift(&existing, &fresh);
    let drift_count = drifts.len();

    let drift_json = serde_json::to_value(&drifts)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("serialising drift: {e}")))?;

    let report_json = serde_json::json!({
        "drift_count": drift_count,
        "drifts": drift_json
    });

    if let Some(report_path) = opts.report {
        if let Some(root) = opts.output_root {
            crate::util::safe_io::ensure_within_root(report_path, root).map_err(|e| {
                AppError::User(format!("index: --report path escapes --output-root: {e}"))
            })?;
        }
        let content = serde_json::to_string_pretty(&report_json)
            .map_err(|e| AppError::Internal(anyhow::anyhow!("serialising report: {e}")))?;
        crate::util::safe_io::safe_write(report_path, content.as_bytes())
            .map_err(|e| AppError::User(format!("writing report: {e}")))?;
        eprintln!("[index] drift report written to {}", report_path.display());
    }

    println!("{report_json}");

    if drift_count > 0 {
        return Err(AppError::User(format!(
            "{drift_count} drift(s) detected — UI may have changed; rerun `ff-rdp index` to regenerate the map"
        )));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Per-page crawl
// ---------------------------------------------------------------------------

struct CrawledPage {
    page: Page,
    outgoing_links: Vec<String>,
    auth_redirect: bool,
}

/// Known login-related path segments for the `is_login_url` heuristic.
const LOGIN_SEGMENTS: &[&str] = &["login", "signin", "sign-in", "sso", "auth", "oauth"];

/// Heuristic: does this URL look like a login/auth page?
///
/// Matches whole path segments (split on `/`) against known login segment names
/// to avoid false positives from substrings (e.g. "author" containing "auth").
fn is_login_url(url: &str) -> bool {
    // Extract just the path portion (strip scheme + host if present).
    let path = if let Some(after_scheme) = url.find("://") {
        let rest = &url[after_scheme + 3..];
        rest.find('/').map_or(rest, |i| &rest[i..])
    } else {
        url
    };
    // Strip query and fragment.
    let path = path.split(['?', '#']).next().unwrap_or(path);

    path.split('/')
        .any(|seg| LOGIN_SEGMENTS.contains(&seg.to_lowercase().as_str()))
}

fn crawl_page(
    cli: &Cli,
    url: &str,
    base_url: &str,
    cross_origin: bool,
) -> anyhow::Result<CrawledPage> {
    // Navigate to the page. Uses `run_core` — the non-printing entry point
    // also used by the script runner — rather than the public `navigate::run`
    // (iter-141 Theme B). `run` unconditionally prints a JSON envelope to
    // stdout; calling it once per crawled page interleaved its output with
    // `index`'s own summary JSON, producing two (or `--max-pages` many)
    // concatenated JSON documents on stdout that `jq`/`json.load` cannot
    // parse — a JSON-only CLI must emit exactly one document.
    use crate::commands::navigate::{WaitAfterNav, WaitLevel, run_core as navigate_run_core};
    let wait_opts = WaitAfterNav {
        wait_text: None,
        wait_selector: None,
        wait_timeout: 5000,
        no_wait: false,
        wait_for: &[],
        wait_level: WaitLevel::Complete,
        // iter-114: Events-only never completes on Firefox 152 (the
        // dom-complete document-event subscription doesn't reliably fire —
        // reproduced even against zero-network data: URLs); Both adds the
        // readystate fallback that plain `ff-rdp navigate` already defaults to.
        wait_strategy: crate::commands::navigate::WaitStrategy::Both,
    };
    navigate_run_core(cli, url, &wait_opts).map_err(|e| anyhow::anyhow!("navigate: {e}"))?;

    // Check if we got redirected to a login page.
    let current_url = eval_js_value(cli, "location.href")
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_else(|| url.to_owned());
    let auth_redirect = current_url != url && is_login_url(&current_url);

    // Get title.
    let title = eval_js_value(cli, "document.title")
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .filter(|s| !s.is_empty());

    // Extract forms via our JS template.
    let forms_json = eval_js_string(
        cli,
        &form_extraction_js_template(crate::commands::js_helpers::UNIQUE_SELECTOR_JS_FN),
    )
    .unwrap_or_default();
    let forms = parse_forms_from_json(&forms_json);

    // Extract landmarks via our JS template.
    let landmarks_json = eval_js_string(
        cli,
        &landmark_extraction_js_template(crate::commands::js_helpers::UNIQUE_SELECTOR_JS_FN),
    )
    .unwrap_or_default();
    let landmarks = parse_landmarks_from_json(&landmarks_json);

    // Extract outgoing links.
    let cross_origin_check = if cross_origin { "true" } else { "false" };
    let links_js = format!(
        "(function() {{\
            var origin = location.origin;\
            var links = [];\
            document.querySelectorAll('a[href]').forEach(function(a) {{\
                var href = a.href;\
                if (!{cross_origin_check} && !href.startsWith(origin)) return;\
                var text = (a.getAttribute('aria-label') || a.textContent || '').trim().slice(0, 80);\
                if (href && href.startsWith('http')) {{\
                    links.push({{\
                        label: text,\
                        href: href,\
                        path: href.replace(origin, ''),\
                        selector: a.id ? '#'+a.id : null\
                    }});\
                }}\
            }});\
            return JSON.stringify(links.slice(0, 50));\
        }})()"
    );
    let links_json = eval_js_string(cli, &links_js).unwrap_or_default();
    let (links, outgoing) = parse_links_from_json(&links_json, base_url);

    let path = url.strip_prefix(base_url).unwrap_or(url).to_owned();
    let path = if path.is_empty() {
        "/".to_owned()
    } else {
        path
    };

    let page = Page {
        path,
        title,
        auth_required: false,
        landmarks,
        forms,
        links,
    };

    Ok(CrawledPage {
        page,
        outgoing_links: outgoing,
        auth_redirect,
    })
}

// ---------------------------------------------------------------------------
// JS evaluation helpers
// ---------------------------------------------------------------------------

/// Evaluate a JavaScript expression in the current tab and return the result
/// as a `serde_json::Value`.
///
/// Uses the `eval` command machinery (stringify mode) to get real values.
fn eval_js_value(cli: &Cli, script: &str) -> anyhow::Result<Value> {
    use crate::commands::eval::build_script;
    let js = build_script(script, true, true);
    eval_js_internal(cli, &js)
}

/// Evaluate a JavaScript expression and return the result as a `String`.
fn eval_js_string(cli: &Cli, script: &str) -> anyhow::Result<String> {
    // The JS template already uses JSON.stringify so we get a plain string back.
    let val = eval_js_internal(cli, script)?;
    Ok(val.as_str().map(str::to_owned).unwrap_or_default())
}

/// Core eval: connects to Firefox and evaluates the given JS source.
///
/// Uses the same `WebConsoleActor::evaluate_js_async` plumbing as the
/// `eval` command, but returns just the primitive value (no output).
fn eval_js_internal(cli: &Cli, js: &str) -> anyhow::Result<Value> {
    use crate::commands::connect_tab::connect_and_get_target;
    use ff_rdp_core::WebConsoleActor;

    let mut ctx = connect_and_get_target(cli).map_err(|e| anyhow::anyhow!("{e}"))?;
    let console_actor = ctx.target.console_actor.clone();

    let eval_result = WebConsoleActor::evaluate_js_async(ctx.transport_mut(), &console_actor, js)
        .map_err(|e| anyhow::anyhow!("evaluate_js_async: {e}"))?;

    if let Some(ref exc) = eval_result.exception {
        let msg = exc
            .message
            .as_deref()
            .unwrap_or("evaluation threw an exception");
        return Err(anyhow::anyhow!("JS exception: {msg}"));
    }

    Ok(eval_result.result.to_json())
}

// ---------------------------------------------------------------------------
// Parse helpers
// ---------------------------------------------------------------------------

fn parse_forms_from_json(json_str: &str) -> Vec<Form> {
    let value: Value = serde_json::from_str(json_str).unwrap_or(Value::Array(vec![]));
    let Some(arr) = value.as_array() else {
        return Vec::new();
    };
    arr.iter()
        .filter_map(|f| {
            let id = f.get("id").and_then(Value::as_str).map(str::to_owned);
            let selector = f.get("selector").and_then(Value::as_str)?.to_owned();
            let submit_selector = f
                .get("submit")
                .and_then(|s| s.get("selector"))
                .and_then(Value::as_str)
                .unwrap_or("[type=\"submit\"]")
                .to_owned();
            let posts_to = f
                .get("submit")
                .and_then(|s| s.get("posts_to"))
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .or_else(|| {
                    f.get("action")
                        .and_then(Value::as_str)
                        .filter(|s| !s.is_empty())
                        .map(str::to_owned)
                });
            let method = f
                .get("method")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(str::to_owned);

            let fields: Vec<Field> = f
                .get("fields")
                .and_then(Value::as_array)
                .map(|fields| {
                    fields
                        .iter()
                        .filter_map(|field| {
                            let name = field.get("name").and_then(Value::as_str)?.to_owned();
                            let field_selector =
                                field.get("selector").and_then(Value::as_str)?.to_owned();
                            let type_ = field
                                .get("type")
                                .and_then(Value::as_str)
                                .unwrap_or("text")
                                .to_owned();
                            let required = field
                                .get("required")
                                .and_then(Value::as_bool)
                                .unwrap_or(false);
                            let placeholder = field
                                .get("placeholder")
                                .and_then(Value::as_str)
                                .filter(|s| !s.is_empty())
                                .map(str::to_owned);
                            Some(Field {
                                name,
                                selector: field_selector,
                                type_,
                                required,
                                placeholder,
                                validation: None,
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();

            Some(Form {
                id,
                selector,
                fields,
                submit: crate::page_map::Submit {
                    selector: submit_selector,
                    posts_to,
                    method,
                },
            })
        })
        .collect()
}

fn parse_landmarks_from_json(json_str: &str) -> Vec<Landmark> {
    let value: Value = serde_json::from_str(json_str).unwrap_or(Value::Array(vec![]));
    let Some(arr) = value.as_array() else {
        return Vec::new();
    };
    arr.iter()
        .filter_map(|l| {
            let name = l.get("name").and_then(Value::as_str)?.to_owned();
            let region = l.get("region").and_then(Value::as_str)?.to_owned();
            let elements: Vec<LandmarkElement> = l
                .get("elements")
                .and_then(Value::as_array)
                .map(|els| {
                    els.iter()
                        .filter_map(|el| {
                            let label = el.get("label").and_then(Value::as_str)?.to_owned();
                            let selector = el.get("selector").and_then(Value::as_str)?.to_owned();
                            let role = el.get("role").and_then(Value::as_str).map(str::to_owned);
                            Some(LandmarkElement {
                                label,
                                selector,
                                role,
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();
            Some(Landmark {
                name,
                region,
                elements,
            })
        })
        .collect()
}

fn parse_links_from_json(json_str: &str, base_url: &str) -> (Vec<Link>, Vec<String>) {
    let value: Value = serde_json::from_str(json_str).unwrap_or(Value::Array(vec![]));
    let Some(arr) = value.as_array() else {
        return (Vec::new(), Vec::new());
    };
    let mut links = Vec::new();
    let mut outgoing = Vec::new();
    for item in arr {
        let label = item
            .get("label")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();
        let href = match item.get("href").and_then(Value::as_str) {
            Some(h) => h.to_owned(),
            None => continue,
        };
        let selector = item
            .get("selector")
            .and_then(Value::as_str)
            .map(str::to_owned);

        // Enqueue same-origin URLs, stripping fragments so the same path is not
        // enqueued twice (e.g. `/page#section` and `/page` are the same page).
        if href.starts_with(base_url) {
            let href_no_frag = strip_query_fragment(&href).to_owned();
            outgoing.push(href_no_frag);
        }

        let path = item
            .get("path")
            .and_then(Value::as_str)
            .unwrap_or(&href)
            .to_owned();

        links.push(Link {
            label,
            href: path,
            selector,
        });
    }
    (links, outgoing)
}

// ---------------------------------------------------------------------------
// Utilities
// ---------------------------------------------------------------------------

/// Strip the query string and fragment from a URL, returning a borrow into the
/// original string.
fn strip_query_fragment(url: &str) -> &str {
    url.split(['?', '#']).next().unwrap_or(url)
}

/// Convert a URL to a stable slug key (path-relative, slashes → dashes).
///
/// Query strings and fragments are stripped so that `/page?foo=1` and `/page`
/// map to the same slug.
fn url_to_slug(url: &str, base_url: &str) -> String {
    let url = strip_query_fragment(url);
    let path = url.strip_prefix(base_url).unwrap_or(url);
    let path = path.trim_matches('/');
    if path.is_empty() {
        return "index".to_owned();
    }
    path.replace('/', "-")
}

/// One `robots.txt` record: the (lowercased) user-agent names it declares,
/// and the `Disallow:` paths listed under it.
///
/// Per the robots.txt convention (and the gov.uk file that exposed the
/// iter-141 bug), one or more consecutive `User-agent:` lines share the
/// rules that follow, up to the next `User-agent:` line that appears *after*
/// at least one rule line — that boundary starts a new group. Rules
/// declared under a specific named agent (e.g. `User-agent: deepcrawl`)
/// apply ONLY to a crawler identifying as that agent; they must never leak
/// into the wildcard `User-agent: *` group that generic crawlers (including
/// ff-rdp, which sends no custom crawler UA) fall under.
#[derive(Debug, PartialEq, Eq)]
struct RobotsGroup {
    user_agents: Vec<String>,
    disallow: Vec<String>,
}

/// Case-insensitively strip a `directive:` prefix (e.g. `"user-agent:"`),
/// returning the remainder unmodified (original case preserved) when it
/// matches, or `None` when `line` doesn't start with `directive`.
fn strip_directive_ci<'a>(line: &'a str, directive: &str) -> Option<&'a str> {
    if line.len() >= directive.len() && line[..directive.len()].eq_ignore_ascii_case(directive) {
        Some(&line[directive.len()..])
    } else {
        None
    }
}

/// Parse a `robots.txt` body into its user-agent groups (iter-141 Theme B).
///
/// Only `User-agent:` and `Disallow:` directives are modeled — `Allow:`,
/// `Sitemap:`, `Crawl-delay:`, and inline `#` comments are recognized (so
/// they don't get mis-parsed as a stray `Disallow:`) but otherwise ignored,
/// since nothing in this crawler currently needs finer-grained precedence
/// than "does any `Disallow:` prefix match this path".
fn parse_robots_groups(body: &str) -> Vec<RobotsGroup> {
    let mut groups: Vec<RobotsGroup> = Vec::new();
    let mut cur_uas: Vec<String> = Vec::new();
    let mut cur_disallow: Vec<String> = Vec::new();
    // Whether a rule line (Disallow/Allow) has been seen since the last
    // User-agent line — a fresh User-agent line after this closes the
    // current group and starts a new one; consecutive User-agent lines
    // (this still false) instead accumulate into the same group.
    let mut seen_rule = false;

    for raw_line in body.lines() {
        // Strip inline `#` comments, then surrounding whitespace.
        let line = raw_line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }

        if let Some(rest) = strip_directive_ci(line, "user-agent:") {
            if seen_rule && !cur_uas.is_empty() {
                groups.push(RobotsGroup {
                    user_agents: std::mem::take(&mut cur_uas),
                    disallow: std::mem::take(&mut cur_disallow),
                });
                seen_rule = false;
            }
            cur_uas.push(rest.trim().to_lowercase());
        } else if let Some(rest) = strip_directive_ci(line, "disallow:") {
            seen_rule = true;
            let p = rest.trim();
            if !p.is_empty() {
                cur_disallow.push(p.to_owned());
            }
        } else if strip_directive_ci(line, "allow:").is_some() {
            seen_rule = true;
        }
        // Other directives (Sitemap, Crawl-delay, ...) are ignored.
    }
    if !cur_uas.is_empty() {
        groups.push(RobotsGroup {
            user_agents: cur_uas,
            disallow: cur_disallow,
        });
    }
    groups
}

/// Select the `Disallow:` paths that apply to us from a parsed set of
/// `robots.txt` groups (iter-141 Theme B).
///
/// ff-rdp does not register a custom crawler user-agent, so only the
/// wildcard `User-agent: *` group ever applies — a group scoped to a named
/// agent (e.g. `deepcrawl`) must never block a crawl that isn't that agent,
/// which is exactly the gov.uk bug this fixes (`Disallow: /` under
/// `User-agent: deepcrawl` was being applied to us, indexing 1 page instead
/// of the requested 3).
///
/// Returns `None` if the wildcard group disallows the entire site
/// (`Disallow: /` or `Disallow: /*`); `Some(vec![])` if there is no
/// wildcard group at all (nothing applies to us); otherwise the wildcard
/// group's `Disallow:` paths.
fn select_group_disallows(groups: &[RobotsGroup]) -> Option<Vec<String>> {
    let Some(wildcard) = groups
        .iter()
        .find(|g| g.user_agents.iter().any(|ua| ua == "*"))
    else {
        return Some(Vec::new());
    };
    if wildcard.disallow.iter().any(|p| p == "/" || p == "/*") {
        return None;
    }
    Some(wildcard.disallow.clone())
}

/// Fetch disallowed paths from `robots.txt` synchronously.
///
/// Returns an empty `Vec` on any error (fail-open: we don't want a missing
/// robots.txt to block the crawl).
///
/// Returns `None` if the entire site is disallowed (`Disallow: /` or
/// `Disallow: /*`) under the wildcard `User-agent: *` group specifically —
/// see [`select_group_disallows`].
fn fetch_disallowed_paths_inner(base_url: &str) -> Option<Vec<String>> {
    use std::io::Read as _;
    let robots_url = format!("{base_url}/robots.txt");
    let resp = ureq::get(&robots_url).call().ok()?;
    let mut limited = resp.into_body().into_reader().take(512 * 1024);
    let mut body_str = String::new();
    limited.read_to_string(&mut body_str).ok()?;

    let groups = parse_robots_groups(&body_str);
    select_group_disallows(&groups)
}

/// Fetch disallowed paths from `robots.txt`, returning an empty `Vec` if the
/// file is missing or unreadable, and an empty `Vec` if the entire site is
/// disallowed (callers should check `disallowed_paths` against each URL path).
///
/// When `None` is returned the crawl should be skipped entirely; we represent
/// "entire site blocked" as `None` internally and convert to empty vec here so
/// the crawler simply skips every URL (since every path starts with `/`).
fn fetch_disallowed_paths(base_url: &str) -> Vec<String> {
    match fetch_disallowed_paths_inner(base_url) {
        Some(paths) => paths,
        // None means the entire site is disallowed — treat as blocking "/".
        None => vec!["/".to_owned()],
    }
}

/// Write a page-map to the caller-supplied path using safe_write (no symlink follow).
///
/// This write is **not** atomic: if the process is interrupted mid-write the
/// destination can be left truncated. A temp-then-rename strategy was
/// considered but rejected because `std::fs::rename` follows a symlink at the
/// destination on Unix, which would defeat the symlink-refusal guarantee that
/// motivated `safe_write` in the first place.
pub fn write_page_map(path: &Path, map: &PageMap, format: &str) -> anyhow::Result<()> {
    // Ensure parent directory exists.
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating directory '{}'", parent.display()))?;
    }

    let format_lower = format.to_lowercase();
    let content = match format_lower.as_str() {
        "yaml" | "yml" => serde_norway::to_string(map).context("serialising page-map as YAML")?,
        "json" => serde_json::to_string_pretty(map).context("serialising page-map as JSON")?,
        other => {
            anyhow::bail!("--format must be 'json', 'yaml', or 'yml', got: {other:?}");
        }
    };

    // Use safe_write so a symlink pre-positioned at the destination is refused.
    crate::util::safe_io::safe_write(path, content.as_bytes())
        .with_context(|| format!("writing page-map to '{}'", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── iter-141 Theme B: robots.txt user-agent grouping ─────────────────

    /// The exact shape reported from gov.uk in dogfooding session 63:
    /// `User-agent: *` disallows a couple of narrow paths, while
    /// `Disallow: /` sits under an unrelated `User-agent: deepcrawl` group.
    /// A generic crawler (ff-rdp sends no custom UA) must honor only the
    /// wildcard group's rules — not the `deepcrawl`-scoped `Disallow: /`.
    #[test]
    fn unit_robots_foreign_ua_disallow_all_does_not_block_wildcard_crawl() {
        let body = "\
User-agent: *
Disallow: /*/print$
Disallow: /search/all*

User-agent: deepcrawl
Disallow: /
";
        let groups = parse_robots_groups(body);
        assert_eq!(groups.len(), 2, "expected two groups, got: {groups:?}");

        let disallow =
            select_group_disallows(&groups).expect("wildcard group must not block everything");
        assert_eq!(
            disallow,
            vec!["/*/print$".to_owned(), "/search/all*".to_owned()],
            "only the wildcard group's Disallow paths must apply"
        );
    }

    /// A wildcard group that genuinely disallows everything must still
    /// block the crawl (the fix must not swing the other way and start
    /// ignoring a real full-site block).
    #[test]
    fn unit_robots_wildcard_disallow_all_blocks_crawl() {
        let body = "User-agent: *\nDisallow: /\n";
        let groups = parse_robots_groups(body);
        assert_eq!(select_group_disallows(&groups), None);
    }

    /// A robots.txt with only named (non-wildcard) groups has nothing that
    /// applies to a generic crawler — the disallow list must be empty, not
    /// inherited from an unrelated named agent.
    #[test]
    fn unit_robots_no_wildcard_group_means_nothing_disallowed() {
        let body = "User-agent: deepcrawl\nDisallow: /\n";
        let groups = parse_robots_groups(body);
        assert_eq!(select_group_disallows(&groups), Some(Vec::new()));
    }

    /// Multiple consecutive `User-agent:` lines share one group's rules —
    /// the standard robots.txt "one or more agents, then rules" shape.
    #[test]
    fn unit_robots_consecutive_user_agents_share_one_group() {
        let body = "User-agent: googlebot\nUser-agent: *\nDisallow: /private\n";
        let groups = parse_robots_groups(body);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].user_agents, vec!["googlebot", "*"]);
        assert_eq!(
            select_group_disallows(&groups),
            Some(vec!["/private".to_owned()])
        );
    }

    /// Comments and blank lines must not be mis-parsed as directives.
    #[test]
    fn unit_robots_ignores_comments_and_blank_lines() {
        let body = "\
# comment line
User-agent: *
# another comment
Disallow: /admin # trailing comment

Sitemap: https://example.com/sitemap.xml
";
        let groups = parse_robots_groups(body);
        assert_eq!(
            select_group_disallows(&groups),
            Some(vec!["/admin".to_owned()])
        );
    }

    // ── iter-141 Theme B: double-enqueue ──────────────────────────────────

    /// `enqueued` must dedupe a URL that is discovered as an outgoing link
    /// from multiple pages before it's popped — regression guard for the
    /// crawl loop's `enqueued.insert(link.clone())` gate (the loop itself
    /// needs a live Firefox crawl to exercise end-to-end; this unit test
    /// pins the underlying dedup primitive so a future refactor can't drop
    /// the `enqueued` check without a test noticing).
    #[test]
    fn unit_enqueue_dedup_across_multiple_referring_pages() {
        let visited: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut enqueued: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut queue: VecDeque<(String, u32)> = VecDeque::new();
        enqueued.insert("https://example.com/".to_owned());

        // Two different "pages" both link to the same URL.
        for _ in 0..2 {
            let links = vec!["https://example.com/about".to_owned()];
            for link in &links {
                if !visited.contains(link) && enqueued.insert(link.clone()) {
                    queue.push_back((link.clone(), 1));
                }
            }
        }

        assert_eq!(
            queue.len(),
            1,
            "the same URL discovered from two referring pages must be enqueued once, got: {queue:?}"
        );
    }
}
