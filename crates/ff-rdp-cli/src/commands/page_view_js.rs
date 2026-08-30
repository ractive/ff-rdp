//! The in-page JavaScript behind the page view (iter-210 Theme A, extended by
//! iter-219 Theme B/C/D).
//!
//! Split out of [`super::page_view`] because the reader-view work turned one
//! 60-line template into three: the injection payload that ships Mozilla's
//! `Readability.js` into the document, the collector that runs against it, and
//! the hand-rolled JSON writer both use. The Rust that *interprets* the result
//! stays next door; this module is only the payload.
//!
//! # Why a hand-rolled JSON writer
//!
//! The collector runs in the page's own realm, and a page is free to replace
//! `JSON.stringify`, `Array.prototype.push` or `Array.prototype.forEach` with
//! something that lies (ad and analytics bundles do it by accident, a hostile
//! page does it on purpose). Every built-in this payload depends on is either
//! captured at injection time on the closure-held handle or avoided outright:
//! arrays are appended with `a[a.length] = x`, iteration is `for (var i = 0;
//! …)`, and the result is serialised by [`JSON_WRITER_JS`] rather than by
//! `JSON.stringify`. That is what makes `meta.page_parse_ms` and the zones
//! trustworthy on a page nobody vetted.

/// The vendored Readability bundle, pinned by `xtask check-vendored-js`.
///
/// Minified (≈32 KB) because it is evaluated over the wire: the unminified
/// copy is committed beside it for diagnosis but never shipped to the page.
pub(crate) const READABILITY_MIN_JS: &str = include_str!("../../js/readability/Readability.min.js");

/// `isProbablyReaderable`, the cheap "does this document look like an article"
/// predicate that becomes `page.readerable`.
pub(crate) const READERABLE_JS: &str = include_str!("../../js/readability/Readability-readerable.js");

/// The attribute the collector stamps on every interactive element so the
/// article-containment test is exact.
///
/// Removed in a `finally` before the collector returns — the live DOM is
/// byte-identical afterwards, which `live_219_reader_view` asserts by
/// comparing `documentElement.outerHTML` either side of a `--with-page` call.
/// Matching by `href` instead was rejected: Wikipedia links the same article
/// from the infobox, the body and a navbox, and `href="#"` buttons collide
/// wholesale.
pub(crate) const STAMP_ATTR: &str = "data-ffrdp-id";

/// The property the injected bundle hangs off `window`.
///
/// Defined with `writable: false, configurable: false` so a page cannot swap
/// the handle out from under a later call; the collector re-validates `v`
/// before trusting it and asks for a re-injection if anything looks wrong.
pub(crate) const HANDLE_PROP: &str = "__ffrdpReaderView";

/// Handle-shape version. Bump when the injected surface changes so a document
/// that survived an ff-rdp upgrade (a long-lived tab) gets re-injected instead
/// of being read through a stale handle.
pub(crate) const HANDLE_VERSION: u32 = 1;

/// A tamper-proof JSON writer for the shapes this collector builds.
///
/// Only strings, finite numbers, booleans, null, arrays and plain objects
/// occur, so this is deliberately not a general `JSON.stringify` replacement.
/// Lone surrogates and U+2028/U+2029 are escaped, which keeps the payload
/// valid JSON even when a page title contains them.
const JSON_WRITER_JS: &str = r#"
  var __ffrdpHasOwn = Object.prototype.hasOwnProperty;
  function __ffrdpQuote(s) {
    var out = '"';
    for (var qi = 0; qi < s.length; qi++) {
      var c = s.charCodeAt(qi);
      if (c === 34) { out += '\\"'; }
      else if (c === 92) { out += '\\\\'; }
      else if (c === 10) { out += '\\n'; }
      else if (c === 13) { out += '\\r'; }
      else if (c === 9) { out += '\\t'; }
      else if (c === 8) { out += '\\b'; }
      else if (c === 12) { out += '\\f'; }
      else if (c < 32 || c === 0x2028 || c === 0x2029 || (c >= 0xD800 && c <= 0xDFFF)) {
        var h = c.toString(16);
        while (h.length < 4) { h = '0' + h; }
        out += '\\u' + h;
      } else { out += s.charAt(qi); }
    }
    return out + '"';
  }
  function __ffrdpJson(v) {
    if (v === null || v === undefined) { return 'null'; }
    var t = typeof v;
    if (t === 'string') { return __ffrdpQuote(v); }
    if (t === 'boolean') { return v ? 'true' : 'false'; }
    if (t === 'number') { return (v === v && v !== Infinity && v !== -Infinity) ? String(v) : 'null'; }
    if (Object.prototype.toString.call(v) === '[object Array]') {
      var parts = '';
      for (var ai = 0; ai < v.length; ai++) {
        if (ai > 0) { parts += ','; }
        parts += __ffrdpJson(v[ai]);
      }
      return '[' + parts + ']';
    }
    var body = '';
    var first = true;
    for (var k in v) {
      if (!__ffrdpHasOwn.call(v, k)) { continue; }
      if (v[k] === undefined) { continue; }
      if (!first) { body += ','; }
      first = false;
      body += __ffrdpQuote(k) + ':' + __ffrdpJson(v[k]);
    }
    return '{' + body + '}';
  }
"#;

/// The injection payload: evaluate the vendored bundle once per document and
/// park it on a frozen, non-configurable handle.
///
/// The bundle's two files declare `function Readability` and `function
/// isProbablyReaderable` at the top level of *this* IIFE, so nothing but the
/// handle escapes into the page's global scope — a page cannot shadow
/// `Readability` and change what ff-rdp reports. `performance.now` is captured
/// here too: the timing in `meta.page_parse_ms` is measured with the clock the
/// page had at injection time, not one it swapped in afterwards.
///
/// Returns nothing useful; callers concatenate it in front of the collector so
/// injection and collection are one round trip.
pub(crate) fn build_injection_js() -> String {
    format!(
        r#"(function() {{
  try {{
    var existing = window.{prop};
    if (existing && existing.v === {version}) {{ return; }}
  }} catch (e) {{ /* a getter that throws is treated as "not injected" */ }}
{readability}
;
{readerable}
;
  var perf = (window.performance && typeof window.performance.now === 'function')
    ? function() {{ return window.performance.now(); }}
    : function() {{ return 0; }};
  var handle = Object.freeze({{
    v: {version},
    Readability: Readability,
    isProbablyReaderable: isProbablyReaderable,
    now: perf
  }});
  try {{
    Object.defineProperty(window, '{prop}', {{
      value: handle, writable: false, configurable: false, enumerable: false
    }});
  }} catch (e) {{
    /* Already defined and non-configurable — a previous injection won the
       race. Leave it: re-defining is what would throw, and the existing
       handle is by construction the same bundle. */
  }}
}})()"#,
        prop = HANDLE_PROP,
        version = HANDLE_VERSION,
        readability = READABILITY_MIN_JS,
        readerable = READERABLE_JS,
    )
}

/// The collector template.
///
/// `__UNIQUE_SELECTOR_FN__` / `__ACC_NAME_FN__` / `__JSON_WRITER__` are spliced
/// by [`build_page_view_js`]; `__LANDMARKS_BLOCK__` and `__READER_BLOCK__` are
/// spliced or emptied depending on the caller, so `a11y summary` pays for
/// neither the reader pass nor a Readability round trip it has no use for.
const PAGE_VIEW_JS_TEMPLATE: &str = r#"(function() {
  __UNIQUE_SELECTOR_FN__
  __ACC_NAME_FN__
  __JSON_WRITER__
  var result = {headings: [], interactive: []};
  var __els = [];
  function __ffrdpAdd(el, entry) {
    entry.__resolver = __ffrdpUniqueSelector(el);
    __els[__els.length] = el;
    result.interactive[result.interactive.length] = entry;
  }

__LANDMARKS_BLOCK__

  // Headings
  for (var level = 1; level <= 6; level++) {
    var headings = document.querySelectorAll('h' + level);
    for (var j = 0; j < headings.length; j++) {
      result.headings[result.headings.length] = {level: level, text: __ffrdpAccName(headings[j])};
    }
  }

  // Interactive: links
  var links = document.querySelectorAll('a[href]');
  for (var k = 0; k < links.length; k++) {
    __ffrdpAdd(links[k], {role: 'link', name: __ffrdpAccName(links[k]),
      href: links[k].getAttribute('href')});
  }

  // Interactive: buttons
  var buttons = document.querySelectorAll('button, [role="button"], input[type="button"], input[type="submit"]');
  for (var m = 0; m < buttons.length; m++) {
    __ffrdpAdd(buttons[m], {role: 'button', name: __ffrdpAccName(buttons[m])});
  }

  // Interactive: inputs (text, email, password, etc.)
  var inputs = document.querySelectorAll('input:not([type="button"]):not([type="submit"]):not([type="hidden"]), textarea, select');
  for (var n = 0; n < inputs.length; n++) {
    var inp = inputs[n];
    __ffrdpAdd(inp, {role: 'input',
      name: __ffrdpAccName(inp) || inp.getAttribute('name') || '',
      type: inp.getAttribute('type') || inp.tagName.toLowerCase()});
  }

__READER_BLOCK__

  return '__FF_RDP_JSON__' + __ffrdpJson(result);
})()"#;

/// Landmark collection — `a11y summary`'s section, and only its.
///
/// iter-219 Theme B drops it from `--with-page`: on Wikipedia it is 22 entries
/// of `{"role":"navigation","label":""}` that no benchmark trajectory ever
/// read, and `--with-page` rides along with every click. `a11y summary` is the
/// accessibility surface and keeps them.
const LANDMARKS_BLOCK_JS: &str = r#"
  result.landmarks = [];
  var landmarkRoles = ['banner','navigation','main','contentinfo','complementary','search','form'];
  for (var lr = 0; lr < landmarkRoles.length; lr++) {
    var roleEls = document.querySelectorAll('[role="' + landmarkRoles[lr] + '"]');
    for (var ri = 0; ri < roleEls.length; ri++) {
      result.landmarks[result.landmarks.length] = {
        role: landmarkRoles[lr],
        label: roleEls[ri].getAttribute('aria-label') || '',
        tag: roleEls[ri].tagName.toLowerCase()
      };
    }
  }
  var landmarkTags = ['HEADER','NAV','MAIN','FOOTER','ASIDE'];
  var landmarkTagRoles = ['banner','navigation','main','contentinfo','complementary'];
  for (var lt = 0; lt < landmarkTags.length; lt++) {
    var tagEls = document.getElementsByTagName(landmarkTags[lt]);
    for (var ti = 0; ti < tagEls.length; ti++) {
      if (tagEls[ti].getAttribute('role')) { continue; }
      result.landmarks[result.landmarks.length] = {
        role: landmarkTagRoles[lt],
        label: tagEls[ti].getAttribute('aria-label') || '',
        tag: landmarkTags[lt].toLowerCase()
      };
    }
  }
"#;

/// The reader pass: stamp, clone, parse, zone, extract text, unstamp.
///
/// `__STAMP__`, `__HANDLE__`, `__HANDLE_VERSION__` and `__TEXT_BUDGET__` are
/// substituted by [`build_page_view_js`].
///
/// Three properties this block must hold, each of which has a live test:
///
/// * **The live DOM is unchanged.** The stamp loop is wrapped in
///   `try/finally`; the `finally` removes every attribute it added, including
///   when Readability throws.
/// * **Zones are exact.** Containment is decided by looking the stamp up
///   inside the parsed article element, not by matching names or hrefs.
/// * **The text is bounded.** `__TEXT_BUDGET__` characters cross the wire at
///   most; `text_chars` always reports the full article length so Rust can say
///   whether the excerpt was cut.
const READER_BLOCK_JS: &str = r#"
  var H = null;
  try { H = window.__HANDLE__; } catch (e) { H = null; }
  if (!H || H.v !== __HANDLE_VERSION__ || typeof H.Readability !== 'function') {
    result.reader_missing = true;
  } else {
    var STAMP = '__STAMP__';
    var t0 = H.now();
    var article = null;
    try {
      for (var si = 0; si < __els.length; si++) {
        try { __els[si].setAttribute(STAMP, String(si)); } catch (e) { /* detached */ }
      }
      var clone = document.cloneNode(true);
      try {
        article = new H.Readability(clone, {
          serializer: function(el) { return el; },
          keepClasses: true
        }).parse();
      } catch (e) {
        article = null;
        result.reader_error = String((e && e.message) || e);
      }
    } finally {
      for (var ui = 0; ui < __els.length; ui++) {
        try { __els[ui].removeAttribute(STAMP); } catch (e) { /* detached */ }
      }
    }
    result.parse_ms = Math.round((H.now() - t0) * 10) / 10;
    try { result.readerable = !!H.isProbablyReaderable(document); }
    catch (e) { result.readerable = false; }

    var contentRoot = (article && article.content && article.content.querySelectorAll)
      ? article.content : null;
    if (contentRoot) {
      result.source = 'readability';
      if (article.title) { result.title = String(article.title); }
      var marked = contentRoot.querySelectorAll('[' + STAMP + ']');
      var inContent = {};
      for (var mi = 0; mi < marked.length; mi++) {
        inContent['k' + marked[mi].getAttribute(STAMP)] = true;
      }
      for (var zi = 0; zi < result.interactive.length; zi++) {
        result.interactive[zi].zone = inContent['k' + zi] ? 'content' : 'chrome';
      }
      __ffrdpSetText(result, __ffrdpBlockText(contentRoot));
    } else {
      // No article: a dashboard, a form, an SPA without prose. Fall back to
      // the main region's rendered text and zone against that region, so the
      // ordering is still better than DOM order wherever a <main> exists.
      result.source = 'innertext';
      var fallbackRoot = document.querySelector('main')
        || document.querySelector('[role="main"]')
        || document.body;
      for (var fi = 0; fi < result.interactive.length; fi++) {
        var inMain = false;
        try { inMain = !!(fallbackRoot && fallbackRoot.contains(__els[fi])); } catch (e) { inMain = false; }
        result.interactive[fi].zone = inMain ? 'content' : 'chrome';
      }
      var raw = '';
      if (fallbackRoot) { raw = fallbackRoot.innerText || fallbackRoot.textContent || ''; }
      __ffrdpSetText(result, __ffrdpNormLines(raw));
    }
  }

  // Whitespace normalisation without `String.prototype.replace` or a regex:
  // collapse every run of whitespace (including NBSP, which MediaWiki emits
  // between a number and its unit) to one space and trim the ends.
  function __ffrdpNorm(s) {
    if (!s) { return ''; }
    var out = '';
    var space = true;
    for (var ni = 0; ni < s.length; ni++) {
      var cc = s.charCodeAt(ni);
      var isSpace = (cc === 32 || cc === 9 || cc === 10 || cc === 13 || cc === 12 ||
                     cc === 11 || cc === 160 || cc === 0xFEFF || cc === 0x200B);
      if (isSpace) { if (!space) { out += ' '; space = true; } }
      else { out += s.charAt(ni); space = false; }
    }
    if (out.charAt(out.length - 1) === ' ') { out = out.substring(0, out.length - 1); }
    return out;
  }

  // Rendered text, one block per line. `textContent` on the article element
  // would run every paragraph together (a detached clone has no layout, so
  // `innerText` degrades to `textContent`), and `--query`'s ±context window is
  // line-based — so the block structure has to be rebuilt explicitly.
  function __ffrdpBlockText(root) {
    // Prose blocks only. Table cells are deliberately excluded: on Wikipedia
    // the infobox is a 1 000-character run of label/value fragments ("Born /
    // Augusta Ada Byron / London, England") that would swallow the whole
    // --page-chars budget before the lede was reached. Infobox *links* are
    // still in `interactive` with zone "content", and `page-text --query`
    // still reaches the cells themselves.
    var SEL = 'p,h1,h2,h3,h4,h5,h6,li,blockquote,pre,dd,dt,figcaption';
    var blocks = root.querySelectorAll(SEL);
    var lines = [];
    for (var bi = 0; bi < blocks.length; bi++) {
      if (blocks[bi].querySelector(SEL)) { continue; }
      // Anything inside a table is layout or an infobox, not prose.
      try { if (blocks[bi].closest('table')) { continue; } } catch (e) { /* old engine */ }
      var t = __ffrdpNorm(blocks[bi].textContent);
      if (t) { lines[lines.length] = t; }
    }
    if (lines.length === 0) {
      var whole = __ffrdpNorm(root.textContent);
      return whole ? whole : '';
    }
    var joined = '';
    for (var li = 0; li < lines.length; li++) {
      if (li > 0) { joined += '\n'; }
      joined += lines[li];
    }
    return joined;
  }

  // `innerText` already carries the block structure; only the runs of blank
  // lines and trailing spaces need flattening.
  function __ffrdpNormLines(s) {
    var raw = String(s || '').split('\n');
    var lines = [];
    for (var ri2 = 0; ri2 < raw.length; ri2++) {
      var t2 = __ffrdpNorm(raw[ri2]);
      if (t2) { lines[lines.length] = t2; }
    }
    var out2 = '';
    for (var oi = 0; oi < lines.length; oi++) {
      if (oi > 0) { out2 += '\n'; }
      out2 += lines[oi];
    }
    return out2;
  }

  // Bound what crosses the wire. `text_chars` is the honest full length; the
  // budget is generous enough that Rust's boundary cut never runs out of
  // material at the requested --page-chars.
  function __ffrdpSetText(res, text) {
    res.text_chars = text.length;
    res.text = text.length > __TEXT_BUDGET__ ? text.substring(0, __TEXT_BUDGET__) : text;
  }
"#;

/// Build the collector JS.
///
/// `landmarks` and `reader` are independent: `a11y summary` takes landmarks
/// without the reader pass, `--with-page` takes the reader pass without
/// landmarks, and the unit tests take every combination.
pub(crate) fn build_page_view_js(landmarks: bool, reader: Option<usize>) -> String {
    // iter-211 Theme C: names come from the shared `__ffrdpAccName` helper —
    // the same one `dom` uses — rather than four hand-rolled
    // `textContent.trim().slice(0, 100)` variants that disagreed with each
    // other and cut real titles mid-word.
    let reader_block = match reader {
        Some(budget) => READER_BLOCK_JS
            .replace("__STAMP__", STAMP_ATTR)
            .replace("__HANDLE__", HANDLE_PROP)
            .replace("__HANDLE_VERSION__", &HANDLE_VERSION.to_string())
            .replace("__TEXT_BUDGET__", &budget.to_string()),
        None => String::new(),
    };
    PAGE_VIEW_JS_TEMPLATE
        .replace(
            "__UNIQUE_SELECTOR_FN__",
            super::js_helpers::UNIQUE_SELECTOR_JS_FN,
        )
        .replace("__ACC_NAME_FN__", &super::js_helpers::acc_name_js_fn())
        .replace("__JSON_WRITER__", JSON_WRITER_JS)
        .replace(
            "__LANDMARKS_BLOCK__",
            if landmarks { LANDMARKS_BLOCK_JS } else { "" },
        )
        .replace("__READER_BLOCK__", &reader_block)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::js_helpers::JSON_SENTINEL;

    #[test]
    fn collector_has_sentinel_and_no_json_stringify() {
        let js = build_page_view_js(true, Some(4000));
        assert!(js.contains(JSON_SENTINEL), "JS must use the sentinel prefix");
        // Theme D: a page that replaced JSON.stringify must not be able to
        // change what ff-rdp reports.
        assert!(js.contains("__ffrdpJson(result)"));
    }

    /// Theme D, the array half: this module's own templates never call a
    /// method a page can replace on `Array.prototype`. The check is scoped to
    /// the templates rather than the assembled script because the shared
    /// `__ffrdpAccName` helper (iter-211, used by `dom` too) predates the
    /// rule — hardening it is a change to `js_helpers`, not to this module,
    /// and would need `dom`'s tests moved with it.
    #[test]
    fn collector_templates_avoid_mutable_array_builtins() {
        for (name, template) in [
            ("PAGE_VIEW_JS_TEMPLATE", PAGE_VIEW_JS_TEMPLATE),
            ("LANDMARKS_BLOCK_JS", LANDMARKS_BLOCK_JS),
            ("READER_BLOCK_JS", READER_BLOCK_JS),
            ("JSON_WRITER_JS", JSON_WRITER_JS),
        ] {
            for banned in [".forEach(", ".push(", ".map(", "Object.keys(", "JSON.stringify"] {
                assert!(
                    !template.contains(banned),
                    "{name} must not depend on {banned} — a page can replace it"
                );
            }
        }
    }

    #[test]
    fn every_placeholder_is_substituted() {
        for (landmarks, reader) in [(true, None), (false, Some(1000)), (true, Some(1000))] {
            let js = build_page_view_js(landmarks, reader);
            for placeholder in [
                "__UNIQUE_SELECTOR_FN__",
                "__ACC_NAME_FN__",
                "__JSON_WRITER__",
                "__LANDMARKS_BLOCK__",
                "__READER_BLOCK__",
                "__STAMP__",
                "__HANDLE_VERSION__",
                "__TEXT_BUDGET__",
            ] {
                assert!(
                    !js.contains(placeholder),
                    "{placeholder} survived substitution (landmarks={landmarks} reader={reader:?})"
                );
            }
        }
    }

    #[test]
    fn landmarks_and_reader_are_independent() {
        let with_page = build_page_view_js(false, Some(2000));
        assert!(!with_page.contains("result.landmarks"), "{with_page}");
        assert!(with_page.contains("reader_missing"));

        let a11y = build_page_view_js(true, None);
        assert!(a11y.contains("result.landmarks"));
        assert!(
            !a11y.contains("reader_missing"),
            "a11y summary must not pay for the reader pass"
        );
        assert!(!a11y.contains(STAMP_ATTR), "no stamping without the reader");
    }

    #[test]
    fn reader_block_strips_its_stamp_in_a_finally() {
        let js = build_page_view_js(false, Some(2000));
        let finally_at = js.find("} finally {").expect("the strip must be in a finally");
        let remove_at = js
            .find("removeAttribute(STAMP)")
            .expect("the stamp must be removed");
        assert!(
            remove_at > finally_at,
            "removeAttribute must sit inside the finally block"
        );
    }

    #[test]
    fn text_budget_reaches_the_payload() {
        let js = build_page_view_js(false, Some(7331));
        assert!(js.contains("> 7331 ?"), "the budget must be spliced in");
    }

    #[test]
    fn injection_defines_a_locked_handle_and_returns_early_when_present() {
        let js = build_injection_js();
        assert!(js.contains("Object.defineProperty(window, '__ffrdpReaderView'"));
        assert!(js.contains("writable: false, configurable: false"));
        assert!(js.contains("if (existing && existing.v === 1) { return; }"));
        // The bundle itself must actually be in there — an `include_str!` that
        // silently resolved to an empty file would otherwise pass every other
        // test in this module.
        assert!(
            js.contains("function Readability("),
            "the vendored bundle must be spliced in"
        );
        assert!(js.contains("function isProbablyReaderable("));
        assert!(js.len() > 30_000, "unexpectedly small payload: {}", js.len());
    }

    /// Nothing but the handle may leak into the page's global scope: the whole
    /// injection is one IIFE, so `Readability` stays a closure binding.
    #[test]
    fn injection_is_a_single_iife() {
        let js = build_injection_js();
        assert!(js.starts_with("(function() {"), "{}", &js[..40]);
        assert!(js.trim_end().ends_with("})()"));
    }
}
