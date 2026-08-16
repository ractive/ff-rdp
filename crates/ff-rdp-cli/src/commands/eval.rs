use std::io::Read;

use anyhow::Context as _;
use ff_rdp_core::{
    ActorId, EvaluateScope, Grip, LongStringActor, ObjectActor, ScopedGrip, TabActor,
    WebConsoleActor, sanitize_for_terminal,
};
use serde_json::json;

use crate::cli::args::Cli;
use crate::error::AppError;
use crate::hints::{HintContext, HintSource};
use crate::output;
use crate::output_pipeline::OutputPipeline;

use super::connect_tab::{connect_and_get_target, register_target_fronts};

/// Load the JavaScript source from exactly one of the three input modes.
///
/// The clap `ArgGroup` constraint guarantees that exactly one of `script`,
/// `file`, or `stdin` is non-empty; this helper defensively errors if that
/// invariant is ever violated.
pub(crate) fn load_script(
    script: Option<&str>,
    file: Option<&str>,
    use_stdin: bool,
) -> Result<String, AppError> {
    let sources =
        usize::from(script.is_some()) + usize::from(file.is_some()) + usize::from(use_stdin);
    if sources == 0 {
        return Err(AppError::User(
            "eval requires a script (positional), --file <PATH>, or --stdin".to_owned(),
        ));
    }
    if sources > 1 {
        return Err(AppError::User(
            "eval accepts only one of: positional <SCRIPT>, --file, --stdin".to_owned(),
        ));
    }

    if let Some(s) = script {
        return Ok(s.to_owned());
    }
    if let Some(path) = file {
        return std::fs::read_to_string(path).map_err(|e| {
            AppError::User(format!("eval: could not read script file '{path}': {e}"))
        });
    }
    // stdin branch.
    let mut buf = String::new();
    std::io::stdin()
        .read_to_string(&mut buf)
        .context("eval: failed to read script from stdin")
        .map_err(AppError::from)?;
    Ok(buf)
}

/// Build the final JS source from the user's script plus the `--stringify` and
/// `--no-isolate` flags.
///
/// # CSP safety — no page `eval()` call (iter-93)
///
/// Firefox's `evaluateJSAsync` routes through `Debugger.evalInGlobal` in
/// `devtools/server/actors/webconsole/eval-with-debugger.js:119-247`, which is
/// **not** subject to page Content Security Policy.  Page CSP restricts `eval()`
/// when called *from within a page script*, but the DevTools evaluator operates
/// at the Debugger API level, outside the page's scripting environment.
///
/// The previous isolation strategy wrapped the user code as
/// `(function() { "use strict"; return eval(<encoded>); })()`.  The outer IIFE
/// is fine — the Debugger evaluates it — but the inner `eval()` *is* a call to
/// the page's `eval` function, which IS blocked by the page's CSP.  That is
/// exactly what produced `EvalError: call to eval() blocked by CSP` on MDN and
/// other strict-CSP sites (dogfooding session 59).
///
/// The fix: drop the `eval()` isolation wrapper entirely.  The user script is
/// sent raw — Firefox evaluates it via the DevTools mechanism, which bypasses
/// page CSP by design.
///
/// # Per-call scope and the `--no-isolate` flag (iter-165)
///
/// Dropping the `eval()` wrapper in iter-93 also dropped the *isolation* it
/// happened to provide, and nothing replaced it: from iter-93 to iter-164 the
/// plain synchronous path sent the user's script to `Debugger.evalInGlobal`
/// verbatim.  That call evaluates in the target global's own lexical
/// environment — it bypasses page CSP, but it does **not** hand each
/// evaluation a fresh scope — so a top-level `const`/`let`/`class` was
/// installed in the tab's global lexical environment and survived until
/// navigation.  Running the same script twice therefore failed the second
/// time with `redeclaration of const x`, while `eval --help` promised the
/// opposite (measured 2026-08-16, see `kb/iterations/iteration-165-*`).
///
/// iter-165 restores the promised contract by wrapping any non-single-
/// expression script in the same value-producing IIFE
/// [`wrap_statements_in_iife`] that `--stringify` (iter-161) and the
/// top-level-`await` path (iter-132) already used — those two paths were
/// already isolated, so the plain synchronous path was the sole exception.
/// A single expression cannot declare anything, so it is still sent verbatim
/// and its completion value is unchanged.
///
/// `--no-isolate` stops being a no-op and becomes the documented opt-out it
/// was originally introduced as (iter-52): with it, the plain synchronous
/// path is sent verbatim again and declarations accumulate in the tab's
/// global lexical environment, which is what someone building state up across
/// several `eval` calls wants.  It cannot un-wrap the `--stringify` or
/// `await` paths — their wraps are a syntactic necessity, not an isolation
/// choice — so declarations never leak there regardless.  Because the flag's
/// pre-165 behaviour was identical to the pre-165 *default*, callers already
/// passing `--no-isolate` see no change at all.
///
/// # Stringify
///
/// `--stringify` wraps the value in `JSON.stringify(...)` so the user gets
/// real values instead of Firefox grip metadata.  The stringify helper does NOT
/// use `eval()` and is therefore unaffected by page CSP.
///
/// iter-161 Theme A: `--stringify` used to splice the user's raw text straight
/// into the helper's argument slot, so it accepted only a single expression —
/// `HELPER(const x = 5; x)` is not JavaScript, and `ff-rdp eval --stringify
/// 'const x = 5; x'` failed with `expected expression, got keyword 'const'`
/// even though bare `eval`, `--file` and `--stdin` all ran the same script
/// fine. [`wrap_stringify`] now routes multi-statement scripts through the
/// same statement-boundary machinery the await wrap uses, so `--stringify`
/// accepts exactly what bare `eval` accepts.
///
/// # Top-level `await` (iter-132 Theme C)
///
/// `Debugger.evalInGlobal` evaluates the submitted script as a plain script,
/// not an async function body, so a bare top-level `await expr` throws
/// `SyntaxError: await is only valid in async functions...` — a friction
/// point agents hit naturally (dogfooding session 62), since `.then()`-based
/// scripts already work: `evaluateJSAsync` awaits a Promise **completion
/// value** before returning it to the caller (the page-await path).
///
/// The fix routes any script containing an `await` keyword through
/// [`wrap_top_level_await`], which turns it into an async-IIFE call
/// expression — i.e. exactly the kind of Promise-returning completion value
/// `evaluateJSAsync` already knows how to await. The evaluation path is the
/// same either way; from the caller's perspective only the previously-broken
/// await scripts start working, nothing else changes.
///
/// iter-142 Theme E fixed two follow-on defects in the same wrap: (1) the
/// single-vs-multi-statement heuristic (used to decide whether the wrap
/// synthesizes a `return`) only recognized `;` as a statement separator, so
/// an ASI-separated (newline-only) multi-statement script like
/// `await Promise.resolve(1)\n42` was misclassified as one expression and
/// wrapped into invalid JS — a syntax error reported past the end of the
/// user's input; (2) even when correctly classified as multi-statement, the
/// wrap never returned anything, so a trailing bare expression silently
/// became `{"type":"undefined"}` instead of its real value. See
/// [`top_level_statement_boundaries`] and [`wrap_top_level_await`].
pub(crate) fn build_script(user_script: &str, stringify: bool, isolate: bool) -> String {
    // The stringify helper: if the value is already a string, return it as-is;
    // otherwise JSON.stringify it. This prevents double-encoding when the JS
    // expression already evaluates to a string (e.g. `document.title`).
    // Circular references throw a TypeError from JSON.stringify; we catch
    // that specific case and return a marker JSON object so the eval still
    // succeeds. All other thrown values (including BigInt's TypeError and
    // Symbol's TypeError) propagate up as eval exceptions.
    const STRINGIFY_HELPER: &str = "(function(v){if(typeof v===\"string\")return v;try{return JSON.stringify(v);}catch(e){if(e instanceof TypeError&&e.message.includes(\"circular\"))return \"{\\\"error\\\":\\\"circular reference detected\\\"}\";throw e;}})";

    let has_await = contains_await_keyword(user_script);

    // Stringify wraps the user's value as the sole argument of a call
    // expression.  Splicing `user_script` there raw is only valid JS when the
    // script is itself a single expression; iter-161 Theme A routes anything
    // else through [`wrap_statements_in_iife`] so what lands in the argument
    // slot is always a single *call* expression.  Either way the wrapped form
    // is a single expression, so `base_is_single_expression` stays `true`.
    let (base, base_is_single_expression) = if stringify {
        (
            wrap_stringify(user_script, STRINGIFY_HELPER, has_await),
            true,
        )
    } else if isolate && !has_await && !looks_like_single_expression(user_script) {
        // iter-165: the plain synchronous path is the only one that used to
        // reach `Debugger.evalInGlobal` unwrapped, so it was the only one
        // whose `const`/`let`/`class` declarations survived into the next
        // call. Wrapping it in the same IIFE the other two paths already use
        // makes the promise in `eval --help` true.
        //
        // `has_await` is excluded because [`wrap_top_level_await`] below runs
        // the very same wrap for those scripts — wrapping here too would
        // nest two IIFEs for no gain.
        (wrap_statements_in_iife(user_script.trim(), false), true)
    } else {
        (
            user_script.to_owned(),
            looks_like_single_expression(user_script),
        )
    };

    if has_await {
        wrap_top_level_await(&base, base_is_single_expression)
    } else {
        base
    }
}

/// Build the `--stringify` wrap: a call to `helper` whose single argument is
/// the user's value (iter-161 Theme A).
///
/// For a single-expression script the shape is unchanged from iter-93 —
/// `(function(){return HELPER(<expr>);})()` — so the common case does not
/// grow an extra IIFE.
///
/// For anything else (declarations, several statements, control flow) the
/// script is first turned into a value-producing IIFE by
/// [`wrap_statements_in_iife`], and *that* call expression becomes the
/// helper's argument:
///
/// ```js
/// (function(){return HELPER((function(){ const x = 5;
/// return (
/// x
/// ); })());})()
/// ```
///
/// `has_await` makes both functions `async` and inserts the `await` that
/// unwraps the inner Promise before it reaches the helper — a synchronous
/// function containing `await` is a SyntaxError, and handing the helper an
/// un-awaited Promise would stringify `{}`. The outer call expression is
/// still a Promise, which is exactly what [`wrap_top_level_await`] and
/// `evaluateJSAsync`'s server-side await expect.
fn wrap_stringify(user_script: &str, helper: &str, has_await: bool) -> String {
    let asyncness = if has_await { "async " } else { "" };
    if looks_like_single_expression(user_script) {
        return format!("({asyncness}function(){{return {helper}({user_script});}})()");
    }
    let inner = wrap_statements_in_iife(user_script.trim(), has_await);
    let argument = if has_await {
        format!("await {inner}")
    } else {
        inner
    };
    format!("({asyncness}function(){{return {helper}({argument});}})()")
}

/// JS identifier keywords that can never begin a bare expression — used by
/// [`looks_like_single_expression`] to reject obvious statement forms.
const STATEMENT_LEADING_KEYWORDS: &[&str] = &[
    "var ", "let ", "const ", "function", "class ", "if ", "if(", "for ", "for(", "while ",
    "while(", "switch ", "switch(", "try", "throw ", "return", "import ", "export ", "do ", "do{",
    "{",
];

/// Best-effort (not a JS parser) check for whether `script` is a single
/// expression, safe to wrap as `return (<script>)` without a syntax error.
///
/// Simplifications, all fail *safe* (degrade to the no-auto-return wrap
/// path in [`wrap_top_level_await`], which still evaluates — it just won't
/// surface a value unless the script has an explicit `return`):
///
/// - A `;`/newline inside a string/template literal is tracked (see
///   [`top_level_statement_boundaries`]) so those don't false-positive as
///   statement separators, but the tracking is char-based, not a real
///   tokenizer — it does not understand regex literals or escaped quotes,
///   so a regex containing `;`/newline-adjacent punctuation could misfire.
/// - Only a fixed, common prefix list is checked against statement-leading
///   keywords; more obscure statement forms (labelled statements, etc.)
///   are not recognized and would be (harmlessly) treated as expressions,
///   which then fail loudly as a SyntaxError from the `return (…)` wrap
///   rather than silently returning the wrong value.
fn looks_like_single_expression(script: &str) -> bool {
    let trimmed = script.trim();
    if trimmed.is_empty() {
        return false;
    }
    let body = trimmed.strip_suffix(';').unwrap_or(trimmed).trim();
    if body.is_empty() {
        return false;
    }
    !looks_like_multi_statement(body)
        && !STATEMENT_LEADING_KEYWORDS
            .iter()
            .any(|kw| body_starts_with_keyword(body, kw))
}

/// Whether `c` can end a complete JS statement/expression on its own —
/// identifiers, numbers, closing brackets, and string terminators all
/// qualify. Used by [`top_level_statement_boundaries`] to judge whether a
/// newline might be an Automatic Semicolon Insertion (ASI) boundary.
fn is_statement_end_char(c: char) -> bool {
    c.is_ascii_alphanumeric()
        || c == '_'
        || c == '$'
        || matches!(c, ')' | ']' | '}' | '\'' | '"' | '`')
}

/// Whether `c` can only appear as a *continuation* of the previous line's
/// expression — a binary operator, member-access `.`, comma, or another
/// closing/continuation character. Seeing one of these as the first
/// character on a new line means the preceding newline is NOT an ASI
/// boundary (real JS ASI famously glues a leading `.`/`+`/`-`/etc. onto the
/// previous statement rather than inserting a semicolon).
fn is_continuation_start_char(c: char) -> bool {
    matches!(
        c,
        '.' | ')'
            | ']'
            | '}'
            | ','
            | '+'
            | '-'
            | '*'
            | '/'
            | '%'
            | '<'
            | '>'
            | '='
            | '!'
            | '&'
            | '|'
            | '^'
            | '?'
            | ':'
            | ';'
    )
}

/// Scan `body` (best-effort — see [`looks_like_single_expression`]'s doc
/// comment for the acknowledged gaps) and return, in order, the byte
/// offsets where a new top-level JS statement appears to begin: one past
/// each top-level `;`, or the first non-whitespace character after each
/// top-level newline that looks like an ASI boundary
/// ([`is_statement_end_char`] before it, NOT [`is_continuation_start_char`]
/// after).
///
/// iter-142 Theme E: the pre-existing `;`-only check missed ASI-separated
/// statements entirely — `await Promise.resolve(1)\n42` has no `;` at all,
/// so it was misclassified as a single expression and wrapped as
/// `return (\nawait Promise.resolve(1)\n42\n)`, which is itself a syntax
/// error (`missing ) in parenthetical`) pointing past the end of the user's
/// input. Newlines are now a statement-separator signal too, gated by
/// [`is_statement_end_char`]/[`is_continuation_start_char`] so common
/// multi-line *single*-expression styles (method chains starting each
/// continuation line with `.`) are not misclassified as multi-statement.
///
/// Tracks single/double-quote and template-literal string state (a `;` or
/// newline *inside* a string is never mistaken for a separator — the old
/// `;`-only check did not do this either) and `(`/`[`/`{` nesting depth (a
/// multi-line object/array literal or argument list is never split).
fn top_level_statement_boundaries(body: &str) -> Vec<usize> {
    #[derive(Clone, Copy, PartialEq)]
    enum Str {
        None,
        Single,
        Double,
        Template,
    }

    let mut boundaries = Vec::new();
    let mut state = Str::None;
    let mut depth: i32 = 0;
    let mut prev_significant: Option<char> = None;
    let chars: Vec<(usize, char)> = body.char_indices().collect();
    let n = chars.len();
    let mut i = 0;

    while i < n {
        let (byte_idx, c) = chars[i];
        match state {
            Str::Single => {
                if c == '\'' {
                    state = Str::None;
                    prev_significant = Some(c);
                }
                i += 1;
                continue;
            }
            Str::Double => {
                if c == '"' {
                    state = Str::None;
                    prev_significant = Some(c);
                }
                i += 1;
                continue;
            }
            Str::Template => {
                if c == '`' {
                    state = Str::None;
                    prev_significant = Some(c);
                }
                i += 1;
                continue;
            }
            Str::None => {}
        }

        match c {
            '\'' => state = Str::Single,
            '"' => state = Str::Double,
            '`' => state = Str::Template,
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth = depth.saturating_sub(1),
            ';' if depth == 0 => boundaries.push(byte_idx + c.len_utf8()),
            '\n' if depth == 0 => {
                let mut j = i + 1;
                while j < n && chars[j].1.is_whitespace() {
                    j += 1;
                }
                if let (Some(prev), Some(&(next_byte, next))) = (prev_significant, chars.get(j))
                    && is_statement_end_char(prev)
                    && !is_continuation_start_char(next)
                {
                    boundaries.push(next_byte);
                }
            }
            _ => {}
        }

        if depth == 0 && state == Str::None && !c.is_whitespace() {
            prev_significant = Some(c);
        }
        i += 1;
    }

    boundaries
}

/// Whether `body` looks like more than one top-level JS statement — see
/// [`top_level_statement_boundaries`].
fn looks_like_multi_statement(body: &str) -> bool {
    !top_level_statement_boundaries(body).is_empty()
}

/// Whether `body` starts with the statement-leading keyword `kw`, at a real
/// word boundary rather than as a bare substring prefix.
///
/// Most [`STATEMENT_LEADING_KEYWORDS`] entries already end in a delimiter
/// (`"if("`, `"let "`, `"do{"`, …), so a plain `starts_with` is inherently
/// boundary-safe for them: the delimiter itself cannot be part of a longer
/// identifier. But a few entries (`"try"`, `"return"`, `"function"`) have no
/// trailing delimiter — both `try{` and `tryFoo()` share that prefix — so
/// for those a plain `starts_with` would misclassify identifiers like
/// `returnValue()` or `tryCatchWrapper()` as statements, silently losing
/// the auto-return optimization for otherwise-ordinary single-expression
/// scripts. Requiring the character right after the keyword (if any) to be
/// a non-identifier character closes that gap without needing a per-keyword
/// trailing-space variant for every bare-word entry.
fn body_starts_with_keyword(body: &str, kw: &str) -> bool {
    let Some(rest) = body.strip_prefix(kw) else {
        return false;
    };
    let is_ident = |c: char| c.is_ascii_alphanumeric() || c == '_' || c == '$';
    // A keyword already ending in a non-identifier delimiter (space, `(`,
    // `{`) needs no further boundary check — the delimiter itself proves
    // the match isn't a longer identifier's prefix.
    if !kw.ends_with(is_ident) {
        return true;
    }
    rest.chars().next().is_none_or(|c| !is_ident(c))
}

/// Whether `script` contains the `await` keyword as a whole identifier
/// token (so `awaited`/`obj.awaitSomething` don't false-positive).
///
/// Does not distinguish a genuinely top-level `await` from one nested
/// inside a user-authored `async function`/arrow body — wrapping the whole
/// script in an outer async IIFE is harmless in the nested case too (see
/// [`build_script`]'s doc comment), so a coarse "does this script use
/// `await` anywhere" check is sufficient to decide whether the wrap is
/// worth applying.
fn contains_await_keyword(script: &str) -> bool {
    let bytes = script.as_bytes();
    let is_ident = |b: u8| b.is_ascii_alphanumeric() || b == b'_' || b == b'$';
    let mut search_from = 0usize;
    while let Some(rel) = script[search_from..].find("await") {
        let start = search_from + rel;
        let end = start + "await".len();
        let before_ok = start == 0 || !is_ident(bytes[start - 1]);
        let after_ok = end == bytes.len() || !is_ident(bytes[end]);
        if before_ok && after_ok {
            return true;
        }
        search_from = start + 1;
    }
    false
}

/// Wrap `base` (the already stringify-processed script) in an async IIFE so
/// its completion value is a Promise `evaluateJSAsync` can await server-side
/// (see [`build_script`]'s "Top-level `await`" doc section).
///
/// When `base` is a single expression, the wrap preserves the pre-await
/// completion-value contract exactly: `return (<base>)`.
///
/// When it is not (multiple statements), a function body has no
/// script-style completion-value semantics — only an explicit `return`
/// inside it produces a value, so the naive wrap (all statements verbatim,
/// no synthesized `return`) silently turned a trailing expression into
/// `undefined` even though the exact same statements evaluated directly (no
/// `await`, no wrap) would have surfaced it as the completion value. This
/// was flagged as the worst failure mode in Theme E: an agent gets
/// `{"type":"undefined"}` with no indication anything went wrong.
///
/// iter-142 fix: if the *last* top-level statement
/// ([`top_level_statement_boundaries`]) is itself a bare expression, split
/// it off and wrap only that part in `return (…)` — every earlier statement
/// still runs unwrapped, so an explicit `return` earlier in the script (a
/// `SyntaxError: Illegal return statement` in top-level script context, so
/// this only appears in scripts that already relied on the `await` wrap)
/// keeps working exactly as before. When the last statement is NOT a bare
/// expression (a declaration, a control-flow construct, an explicit
/// `return` a user already wrote), there is nothing safe to auto-return —
/// the wrap falls back to the no-auto-return form, same as before this
/// iteration.
fn wrap_top_level_await(base: &str, base_is_single_expression: bool) -> String {
    if base_is_single_expression {
        return format!("(async function(){{return (\n{base}\n);}})()");
    }
    wrap_statements_in_iife(base, true)
}

/// Turn a statement sequence into a zero-argument IIFE call expression that
/// evaluates to the value of its last statement.
///
/// Extracted from [`wrap_top_level_await`] in iter-161 so `--stringify`
/// (Theme A) reuses the same statement-boundary machinery instead of adding a
/// second classifier; `is_async` selects between the `async` form the await
/// wrap needs and the plain form `--stringify` uses when no `await` is
/// present.
///
/// If the last top-level statement ([`top_level_statement_boundaries`]) is a
/// bare expression, it is split off and returned via a synthesized
/// `return (…)`; every earlier statement runs verbatim. When it is not (a
/// declaration, a control-flow construct, a `return` the user wrote), there is
/// nothing safe to auto-return and the body is emitted as-is — the script
/// still evaluates, it just yields `undefined` unless the user returns
/// something.
fn wrap_statements_in_iife(body: &str, is_async: bool) -> String {
    let asyncness = if is_async { "async " } else { "" };
    if let Some(split_at) = top_level_statement_boundaries(body).last().copied() {
        let prefix = body[..split_at].trim_end();
        let last = body[split_at..].trim();
        if !last.is_empty() && looks_like_single_expression(last) {
            let last_expr = last.strip_suffix(';').unwrap_or(last).trim();
            return if prefix.is_empty() {
                format!("({asyncness}function(){{return (\n{last_expr}\n);}})()")
            } else {
                format!("({asyncness}function(){{\n{prefix}\nreturn (\n{last_expr}\n);}})()")
            };
        }
    }

    format!("({asyncness}function(){{\n{body}\n}})()")
}

/// Build the final JavaScript source, exposed for use by the script runner.
pub fn build_eval_js(
    script: Option<&str>,
    file: Option<&str>,
    use_stdin: bool,
    stringify: bool,
    no_isolate: bool,
) -> Result<String, AppError> {
    let user_script = load_script(script, file, use_stdin)?;
    let isolate = !no_isolate;
    Ok(build_script(&user_script, stringify, isolate))
}

/// CLI-side companion to [`EvaluateScope`] — owns `&str` slices borrowed
/// from clap so the dispatch site does not have to construct an
/// [`ActorId`] before deciding which connection path to take.
#[derive(Debug, Default, Clone, Copy)]
pub struct CliEvalScope<'a> {
    pub frame_actor: Option<&'a str>,
    pub selected_node_actor: Option<&'a str>,
    pub inner_window_id: Option<u64>,
}

impl CliEvalScope<'_> {
    /// Convert into an owned [`EvaluateScope`] for the core API, returning
    /// `None` when every field is unset (so callers can pass `None` to the
    /// scoped evaluator and stay on the legacy code path).
    pub fn to_scope(self) -> Option<EvaluateScope> {
        if self.frame_actor.is_none()
            && self.selected_node_actor.is_none()
            && self.inner_window_id.is_none()
        {
            return None;
        }
        Some(EvaluateScope {
            frame_actor: self.frame_actor.map(ActorId::from),
            selected_node_actor: self.selected_node_actor.map(ActorId::from),
            inner_window_id: self.inner_window_id,
        })
    }
}

#[allow(clippy::too_many_arguments, clippy::fn_params_excessive_bools)]
pub fn run(
    cli: &Cli,
    script: Option<&str>,
    file: Option<&str>,
    use_stdin: bool,
    stringify: bool,
    no_isolate: bool,
    unwrap: bool,
    cli_scope: CliEvalScope<'_>,
) -> Result<(), AppError> {
    let script = load_script(script, file, use_stdin)?;
    let scope = cli_scope.to_scope();
    // iter-165: `--no-isolate` is honoured again. By default a multi-statement
    // script runs inside a per-call IIFE so its `const`/`let`/`class` never
    // leak into the next `eval`; `--no-isolate` sends it to the shared global
    // lexical environment instead. See [`build_script`]'s doc comment.
    let final_script = build_script(&script, stringify, !no_isolate);

    let mut ctx = connect_and_get_target(cli)?;

    // The console actor ID is taken directly from the target descriptor
    // returned by `get_target`.  The retry path below re-fetches the target
    // if the actor turns out to be stale (noSuchActor / unknownActor).
    let console_actor = ctx.target.console_actor.clone();

    // Evaluate via the DevTools console actor.  Firefox routes this through
    // Debugger.evalInGlobal (eval-with-debugger.js:119-247), which bypasses
    // page CSP — no fallback to a chrome context is needed.
    let eval_result = match WebConsoleActor::evaluate_js_async_scoped(
        ctx.transport_mut(),
        &console_actor,
        &final_script,
        scope.as_ref(),
    ) {
        Ok(result) => result,
        Err(ff_rdp_core::ProtocolError::ActorError {
            kind: ff_rdp_core::ActorErrorKind::UnknownActor,
            ..
        }) => {
            // Actor is stale — re-resolve and retry once.
            let tab_actor = ctx.target_tab_actor().clone();
            let fresh_target =
                TabActor::get_target(ctx.transport_mut(), &tab_actor).map_err(AppError::from)?;
            register_target_fronts(ctx.registry(), &fresh_target);
            let fresh_console = fresh_target.console_actor.clone();
            ctx.target = fresh_target;
            WebConsoleActor::evaluate_js_async_scoped(
                ctx.transport_mut(),
                &fresh_console,
                &final_script,
                scope.as_ref(),
            )
            .map_err(AppError::from)?
        }
        Err(e) => return Err(AppError::from(e)),
    };

    // If an exception occurred, route it through the standard JSON error
    // envelope (iter-141 Theme E) rather than printing bare text to stderr.
    // `eval` is a well-formed-but-invalid-*input* case in exactly the sense
    // Theme E covers for `eval_or_bail`/`poll_js_condition` (invalid CSS
    // selectors, "element not found" polling failures): the script the
    // caller supplied threw, which is on them, not an ff-rdp bug — so this
    // is `AppError::User`, not the `AppError::Exit(1)` that used to bypass
    // `main`'s envelope emission entirely (`ff-rdp eval "throw new
    // Error('x')"` printed `error: x` plus a pretty-JSON dump with no JSON
    // envelope on stdout at all, while every other command failure emits
    // one).
    if let Some(ref exc) = eval_result.exception {
        let msg = exc
            .message
            .as_deref()
            .unwrap_or("evaluation threw an exception");
        return Err(AppError::User(sanitize_for_terminal(msg).into_owned()));
    }

    // Compute the JSON representation before we potentially move the grip into
    // a ScopedGrip.  `to_json()` borrows `result`, so this must come first.
    let mut result_json = eval_result.result.to_json();

    // Wrap object/long-string grips in ScopedGrip so we can release them
    // before the process exits.  Firefox allocates a server-side actor for
    // each such grip returned by evaluateJSAsync; on long-lived daemon
    // connections these accumulate without bound.  We send `release` after
    // printing output so Firefox can free the actor immediately.
    //
    // Release applies equally in direct-connect and daemon-proxy modes: the
    // daemon transparently forwards all RDP frames, so the `release` packet
    // reaches Firefox through the same channel.
    let scoped_grip: Option<ScopedGrip> = match eval_result.result {
        g @ (Grip::Object { .. } | Grip::LongString { .. }) => Some(ScopedGrip::new(g)),
        _ => None,
    };

    // iter-161 Theme C: a string longer than Firefox's ~1000-char inline
    // limit arrives as a `longString` grip carrying only a preview. Every
    // other command resolves that through `js_helpers::resolve_result`;
    // `eval` did not, so it printed the preview as if it were the value —
    // with no `meta.truncated`, no hint, and (because the grip is released a
    // few lines below) no way for the caller to fetch the rest afterwards.
    // Fetch the full string here, while the actor is still alive.
    //
    // `full_string` enforces `LongStringActor::MAX_FETCH` (16 MiB) and its
    // error becomes an `AppError`, so an oversized payload surfaces through
    // the normal JSON error envelope rather than a panic.
    if let Some(ref sg) = scoped_grip
        && let Grip::LongString {
            ref actor, length, ..
        } = *sg.grip()
    {
        let full = LongStringActor::full_string(ctx.transport_mut(), actor.as_ref(), length)
            .map_err(AppError::from)?;
        result_json = serde_json::Value::String(full);
    }

    // For object grips, enrich the output with the list of own property names.
    // Best-effort: if the actor is gone or the request fails, we skip silently.
    //
    // Firefox 149 removed the `ownPropertyNames` packet type, so we use
    // `prototypeAndProperties` and extract the keys from the result.
    if let Some(ref sg) = scoped_grip
        && let Grip::Object { ref actor, .. } = *sg.grip()
    {
        match ObjectActor::prototype_and_properties(ctx.transport_mut(), actor.as_ref()) {
            Ok(pap) => {
                let names: Vec<&str> = pap.own_properties.keys().map(String::as_str).collect();
                result_json["propertyNames"] = json!(names);
            }
            Err(e) => {
                // stderr-ok: (b) best-effort, warn-and-continue — see the
                // comment above; the eval result itself is unaffected.
                eprintln!("warning: could not fetch property names: {e}");
            }
        }
    }

    // When --stringify was used, the JS already ran JSON.stringify() so the
    // eval result is a JSON string (e.g. `"{\"a\":1}"`).  Parse it on the
    // ff-rdp side so `results` holds a real JSON object/array rather than a
    // string — agents can then use `--jq '.results.a'` directly without an
    // extra parse step.
    //
    // If parsing fails (e.g. the expression itself returned a plain string, or
    // the caller double-wrapped via another JSON.stringify), keep the raw
    // string value and set `meta.stringify_parsed: false` so callers know the
    // round-trip did not produce a structured value.
    // iter-161 Theme E: `meta.eval_path` used to be inserted here, hard-set to
    // the constant "page-await". Its only other value ("chrome") was deleted
    // in iter-93 and DEC-020 confirmed it stays deleted, so the field
    // discriminated nothing while reading like a strategy selector. The
    // page-await path itself is unchanged and still documented in
    // `build_script`'s doc comment and in `eval --help`.
    let mut meta = json!({});
    if stringify && let serde_json::Value::String(ref s) = result_json {
        match serde_json::from_str::<serde_json::Value>(s) {
            Ok(parsed) => {
                result_json = parsed;
                // stringify_parsed defaults to true — omit the flag when parsing
                // succeeds so the output stays minimal.
            }
            Err(_) => {
                // Keep the raw string but signal that parsing did not succeed.
                if let Some(m) = meta.as_object_mut() {
                    m.insert("stringify_parsed".to_owned(), json!(false));
                }
            }
        }
    }
    if unwrap
        && try_unwrap_json_string(&mut result_json)
        && let Some(m) = meta.as_object_mut()
    {
        m.insert("unwrapped".to_owned(), json!(true));
    }
    crate::connection_meta::merge_into_if_verbose(
        &mut meta,
        &cli.host,
        cli.port,
        None,
        cli.is_verbose(),
    );
    // iter-134: always present, not gated by --verbose — an
    // agent can tell how this command executed without a
    // separate `daemon status` round-trip.
    crate::connection_meta::merge_route(&mut meta, ctx.via_daemon);
    let envelope = output::envelope(&result_json, 1, &meta);

    let hint_ctx = HintContext::new(HintSource::Eval);
    let pipeline = OutputPipeline::from_cli(cli)?;
    // When the caller passes `--stringify`, they're extracting a raw value;
    // appending the trailing "-> ff-rdp …" hint line would pollute their
    // captured stdout (dogfood-49 #6).  Suppress hints unconditionally in
    // that mode — symmetric with `--jq` and `--no-hints`.
    let pipeline = if stringify {
        pipeline.without_hints()
    } else {
        pipeline
    };
    let pipeline_result = pipeline.finalize_with_hints(&envelope, Some(&hint_ctx));

    // Release the server-side object actor after output is flushed.
    //
    // We intentionally release *after* printing so the caller sees the full
    // output even if release fails.  Release failures are logged at WARN and
    // never propagate — a failed release means the actor leaks until the
    // connection closes, which is acceptable for one-shot CLI invocations.
    if let Some(sg) = scoped_grip
        && let Err(e) = sg.release(ctx.transport_mut())
    {
        tracing::warn!("eval: failed to release object actor: {e}");
    }

    pipeline_result
}

/// `--unwrap` helper: if `value` is a string whose contents parse as a JSON
/// object or array, replace `value` with the parsed structure and return
/// `true`.  Returns `false` and leaves `value` untouched otherwise (including
/// for valid JSON that parses to a primitive — numbers, booleans, null, or
/// plain strings).
fn try_unwrap_json_string(value: &mut serde_json::Value) -> bool {
    let serde_json::Value::String(s) = &*value else {
        return false;
    };
    let Ok(parsed) = serde_json::from_str::<serde_json::Value>(s) else {
        return false;
    };
    if matches!(
        parsed,
        serde_json::Value::Object(_) | serde_json::Value::Array(_)
    ) {
        *value = parsed;
        true
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_script_positional_passthrough() {
        let s = load_script(Some("document.title"), None, false).unwrap();
        assert_eq!(s, "document.title");
    }

    /// AC iter-80 Theme C: `eval_unwrap_parses_json_string`.
    /// A JSON-encoded object string is replaced with the parsed object.
    #[test]
    fn eval_unwrap_parses_json_string() {
        let mut v = serde_json::Value::String(r#"{"a":1}"#.to_owned());
        let unwrapped = try_unwrap_json_string(&mut v);
        assert!(
            unwrapped,
            "expected unwrap to succeed on JSON object string"
        );
        assert_eq!(v, serde_json::json!({"a": 1}));
    }

    /// Negative: a plain string is left unchanged.
    #[test]
    fn eval_unwrap_leaves_plain_string_unchanged() {
        let mut v = serde_json::Value::String("hello".to_owned());
        let unwrapped = try_unwrap_json_string(&mut v);
        assert!(
            !unwrapped,
            "plain non-JSON string must not be unwrapped: {v:?}"
        );
        assert_eq!(v, serde_json::Value::String("hello".to_owned()));
    }

    /// JSON-encoded primitive (e.g. `"42"`) must stay a string — only
    /// objects and arrays unwrap.
    #[test]
    fn eval_unwrap_leaves_primitive_string_unchanged() {
        let mut v = serde_json::Value::String("42".to_owned());
        let unwrapped = try_unwrap_json_string(&mut v);
        assert!(
            !unwrapped,
            "primitive JSON value must not trigger unwrap: {v:?}"
        );
        assert_eq!(v, serde_json::Value::String("42".to_owned()));
    }

    /// Arrays are also valid unwrap targets.
    #[test]
    fn eval_unwrap_parses_json_array_string() {
        let mut v = serde_json::Value::String("[1,2,3]".to_owned());
        let unwrapped = try_unwrap_json_string(&mut v);
        assert!(unwrapped);
        assert_eq!(v, serde_json::json!([1, 2, 3]));
    }

    /// Non-string values are never touched.
    #[test]
    fn eval_unwrap_skips_non_string_values() {
        let mut v = serde_json::json!({"already": "object"});
        let unwrapped = try_unwrap_json_string(&mut v);
        assert!(!unwrapped);
        assert_eq!(v, serde_json::json!({"already": "object"}));
    }

    #[test]
    fn load_script_from_file() {
        let tmp = std::env::temp_dir().join(format!(
            "ff_rdp_eval_{}.js",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(&tmp, "1 + 2").unwrap();
        let s = load_script(None, Some(tmp.to_str().unwrap()), false).unwrap();
        assert_eq!(s, "1 + 2");
        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn load_script_missing_file_is_user_error() {
        let err = load_script(None, Some("/nonexistent/path/xyz.js"), false).unwrap_err();
        // Any AppError variant is fine as long as the message is helpful.
        let msg = format!("{err:?}");
        assert!(
            msg.contains("could not read script file") || msg.contains("xyz.js"),
            "unexpected error: {msg}"
        );
    }

    #[test]
    fn load_script_no_source_errors() {
        let err = load_script(None, None, false).unwrap_err();
        assert!(matches!(err, AppError::User(_)));
    }

    // ---------------------------------------------------------------------------
    // build_script wrapping tests
    // ---------------------------------------------------------------------------

    // ---------------------------------------------------------------------------
    // build_script: iter-93 — no eval() in any code path (CSP safety).
    //
    // Firefox routes evaluateJSAsync through Debugger.evalInGlobal, which
    // bypasses page CSP.  However, any eval() call inside the script IS subject
    // to page CSP.  So build_script must NEVER emit a bare `eval(` substring.
    // ---------------------------------------------------------------------------

    #[test]
    fn build_script_no_isolate_no_stringify_passthrough() {
        let s = build_script("document.title", false, false);
        assert_eq!(s, "document.title");
        // Must not contain a bare eval() call.
        assert!(!s.contains("eval("), "must not contain eval(): {s}");
    }

    #[test]
    fn build_script_isolate_single_expression_is_passthrough() {
        // A single expression declares nothing, so isolation has nothing to
        // do: iter-165 leaves it verbatim and its completion value unchanged.
        let s = build_script("document.title", false, true);
        assert_eq!(s, "document.title");
        assert!(!s.contains("eval("), "must not contain eval(): {s}");
    }

    // ── iter-165: per-call scope on the plain synchronous path ───────────────

    /// The defect this iteration fixes. Before iter-165 the plain path sent
    /// `const x = 1; x` to `Debugger.evalInGlobal` verbatim, so the binding
    /// landed in the tab's global lexical environment and the *second*
    /// identical call died with `redeclaration of const x`. The IIFE wrap
    /// keeps the binding call-local, and the trailing bare expression is
    /// still auto-returned so the value is unchanged.
    #[test]
    fn unit_165_plain_multi_statement_is_wrapped_per_call() {
        let s = build_script("const x = 1; x", false, true);
        assert_eq!(s, "(function(){\nconst x = 1;\nreturn (\nx\n);})()");
        assert!(!s.contains("eval("), "must not contain eval(): {s}");
    }

    /// `let` and `class` share the global lexical environment with `const`,
    /// so all three must be wrapped; `var`/`function` are function-scoped by
    /// the same wrap.
    #[test]
    fn unit_165_plain_wrap_covers_every_declaration_form() {
        for script in [
            "let y = 1; y",
            "var v = 1; v",
            "class C {}; 1",
            "function f(){return 1}; f()",
        ] {
            let s = build_script(script, false, true);
            assert!(
                s.starts_with("(function(){"),
                "{script:?} must be wrapped per call, got: {s}"
            );
            assert!(
                s.contains("return (\n"),
                "{script:?} must keep auto-returning its trailing expression, got: {s}"
            );
        }
    }

    /// `--no-isolate` is honoured again (iter-165): it restores the pre-165
    /// verbatim send, which is what someone deliberately building state up
    /// across calls wants. Its pre-165 behaviour was identical to the pre-165
    /// default, so nobody already passing it sees a change.
    #[test]
    fn unit_165_no_isolate_sends_plain_script_verbatim() {
        let s = build_script("const x = 1; x", false, false);
        assert_eq!(s, "const x = 1; x");
        assert!(!s.contains("eval("), "must not contain eval(): {s}");
    }

    /// `--no-isolate` must not be able to un-wrap the two paths whose wrap is
    /// a syntactic necessity: `--stringify` (iter-161) and top-level `await`
    /// (iter-132). Their declarations stay call-local either way.
    #[test]
    fn unit_165_no_isolate_cannot_unwrap_stringify_or_await() {
        let stringified = build_script("const x = 1; x", true, false);
        assert!(
            stringified.contains("((function(){\nconst x = 1;\nreturn (\nx\n);})())"),
            "--stringify must stay wrapped under --no-isolate: {stringified}"
        );
        let awaited = build_script("const x = await Promise.resolve(1); x", false, false);
        assert!(
            awaited.starts_with("(async function(){"),
            "an await script must stay wrapped under --no-isolate: {awaited}"
        );
    }

    /// An `await` script must not pick up a second, redundant IIFE from the
    /// iter-165 wrap — [`wrap_top_level_await`] already applies exactly the
    /// same one.
    #[test]
    fn unit_165_await_path_is_wrapped_once() {
        let s = build_script("const x = await Promise.resolve(1); x", false, true);
        assert_eq!(
            s,
            "(async function(){\nconst x = await Promise.resolve(1);\nreturn (\nx\n);})()"
        );
    }

    /// AC `unit_165_help_text_matches_behaviour`: the `eval` `long_about` and
    /// [`build_script`] must not drift apart again. Each documented claim is
    /// paired with the behavioural assertion that makes it true, so a change
    /// to either side alone fails this test.
    #[test]
    fn unit_165_help_text_matches_behaviour() {
        use clap::CommandFactory as _;

        let cmd = Cli::command();
        let eval_cmd = cmd
            .get_subcommands()
            .find(|c| c.get_name() == "eval")
            .expect("eval subcommand must exist");
        let help = eval_cmd
            .get_long_about()
            .expect("eval must have a long_about")
            .to_string();

        // Claim: declarations never leak across calls (default path).
        assert!(
            help.contains("never leak across calls"),
            "help must state the per-call-scope contract: {help}"
        );
        assert!(
            build_script("const x = 1; x", false, true).starts_with("(function(){"),
            "the default path must actually isolate, or the claim above is false"
        );

        // Claim: --no-isolate opts out and shares one scope.
        assert!(
            help.contains("--no-isolate to opt out and share ONE scope across calls"),
            "help must document --no-isolate as the opt-out: {help}"
        );
        assert_eq!(
            build_script("const x = 1; x", false, false),
            "const x = 1; x",
            "--no-isolate must actually send the script unwrapped"
        );

        // The pre-165 claim, now false, must not come back.
        assert!(
            !help.contains("is now a no-op"),
            "help must not call --no-isolate a no-op while build_script honours it: {help}"
        );
        assert!(
            !help.contains("each call already has its own scope"),
            "the pre-165 wording asserted Debugger.evalInGlobal gave per-call \
             scope, which it does not — the isolation is ff-rdp's wrap: {help}"
        );

        // Claim: a single expression is sent verbatim.
        assert!(
            help.contains("A single expression declares nothing and is still sent verbatim"),
            "help must document the single-expression passthrough: {help}"
        );
        assert_eq!(
            build_script("document.title", false, true),
            "document.title"
        );
    }

    #[test]
    fn build_script_stringify_only_wraps_in_json_stringify() {
        let s = build_script("document.querySelectorAll('a')", true, false);
        // The stringify helper uses JSON.stringify for non-strings.
        assert!(s.contains("JSON.stringify("));
        assert!(s.contains("document.querySelectorAll('a')"));
        assert!(s.contains("circular"));
        // No bare eval() in any stringify path.
        assert!(!s.contains("eval("), "must not contain eval(): {s}");
        // Strings are passed through without double-encoding.
        assert!(s.contains("typeof v===\"string\""));
    }

    #[test]
    fn build_script_isolate_and_stringify_combine() {
        // isolate=true + stringify=true: result is stringify-wrapped, no eval().
        let s = build_script("document.querySelectorAll('a')", true, true);
        assert!(s.contains("JSON.stringify("));
        assert!(!s.contains("eval("), "must not contain eval(): {s}");
        assert!(s.contains("circular"));
        assert!(s.contains("typeof v===\"string\""));
    }

    #[test]
    fn build_script_stringify_string_passthrough() {
        // When the expression evaluates to a string, the helper must return it
        // without passing through JSON.stringify (no double-encoding).
        let s = build_script("document.title", true, false);
        assert!(s.contains("typeof v===\"string\""));
        // The helper is invoked with the user expression as argument.
        assert!(s.contains("document.title"));
        assert!(!s.contains("eval("), "must not contain eval(): {s}");
    }

    #[test]
    fn build_script_stringify_number_uses_json_stringify() {
        // For non-string values the helper falls through to JSON.stringify.
        let s = build_script("42", true, false);
        assert!(s.contains("JSON.stringify("));
        assert!(s.contains("42"));
        assert!(!s.contains("eval("), "must not contain eval(): {s}");
    }

    #[test]
    fn build_script_handles_special_chars() {
        // Quotes, backslashes, newlines: no-stringify path passes through raw.
        let input = "'a' + \"b\" + `c\nd`";
        let s = build_script(input, false, false);
        assert_eq!(s, input);
        assert!(!s.contains("eval("), "must not contain eval(): {s}");
    }

    /// The four scripts the iter-93 matrix test covered. The live matrix test
    /// `live_161_build_script_matrix_evaluates` repeats this list (it lives in
    /// a separate integration-test crate) and hands each generated script to
    /// Firefox — the only JS parser this repo is allowed to use, since all
    /// code stays in Rust and there is no in-process parser to check "does
    /// this parse" against.
    const MATRIX_SCRIPTS: [&str; 4] = [
        "document.title",
        "1 + 1",
        "const x = 1; x",
        "throw new Error('boom')",
    ];

    /// Invariant: build_script MUST NOT emit a bare `eval(` for any
    /// combination of flags and user input.  This is the CSP-safety invariant
    /// introduced in iter-93.
    ///
    /// iter-161 Theme B: this replaces
    /// `build_script_never_emits_eval_for_any_combination`, which asserted
    /// *only* the `eval(` invariant and therefore passed for
    /// `("const x = 1; x", stringify=true)` while generating the syntactically
    /// invalid JavaScript of Theme A — invalid JS contains no `eval(` either.
    /// The structural assertion below is the part the old test was missing:
    /// for a multi-statement script the helper's argument must be a call
    /// expression, not the user's raw text spliced into an argument slot.
    #[test]
    fn unit_161_build_script_emits_no_bare_eval() {
        for &script in &MATRIX_SCRIPTS {
            for stringify in [false, true] {
                for isolate in [false, true] {
                    let s = build_script(script, stringify, isolate);
                    assert!(
                        !s.contains("eval("),
                        "eval() found in build_script({script:?}, stringify={stringify}, isolate={isolate}): {s}"
                    );
                    if stringify && !looks_like_single_expression(script) {
                        assert!(
                            !s.contains(&format!("}})({script});")),
                            "stringify spliced the raw statement text into the helper's \
                             argument slot for {script:?}: {s}"
                        );
                        assert!(
                            s.contains("})());})()"),
                            "stringify must hand the helper a call expression for \
                             {script:?}: {s}"
                        );
                    }
                }
            }
        }
    }

    /// iter-161 Theme A / AC1: a multi-statement `--stringify` script runs
    /// inside a zero-argument IIFE whose last statement is a synthesized
    /// `return (…)`, and it is that IIFE's *call* that lands in the helper's
    /// argument list — never the raw text.
    #[test]
    fn unit_161_stringify_wraps_multi_statement_in_iife() {
        let s = build_script("const x = 5; x", true, false);
        assert!(
            s.contains("((function(){\nconst x = 5;\nreturn (\nx\n);})())"),
            "expected the statements inside a zero-arg IIFE with a synthesized \
             return, got: {s}"
        );
        assert!(
            !s.contains("})(const x = 5; x)"),
            "the raw statement text must not be spliced into the argument slot: {s}"
        );
        assert!(s.contains("JSON.stringify("));
        assert!(!s.contains("eval("), "must not contain eval(): {s}");
    }

    /// iter-161 Theme A / AC2: the single-expression case — by far the common
    /// one — keeps the exact iter-93 shape. The expected string below is what
    /// `main` produced before this iteration, byte for byte: no extra IIFE.
    #[test]
    fn unit_161_stringify_single_expression_shape_unchanged() {
        let expected = concat!(
            "(function(){return ",
            "(function(v){if(typeof v===\"string\")return v;try{return JSON.stringify(v);}",
            "catch(e){if(e instanceof TypeError&&e.message.includes(\"circular\"))",
            "return \"{\\\"error\\\":\\\"circular reference detected\\\"}\";throw e;}})",
            "(document.title);})()"
        );
        assert_eq!(build_script("document.title", true, false), expected);
    }

    /// iter-161 Theme A: `--stringify` × top-level `await` × several
    /// statements. A synchronous IIFE containing `await` is a SyntaxError, so
    /// the inner value-producing IIFE must be `async` and its Promise must be
    /// awaited before the helper sees it — otherwise the helper stringifies a
    /// pending Promise (`{}`). Pinned as a test rather than argued, per the
    /// plan.
    #[test]
    fn unit_161_stringify_await_multi_statement_is_async_throughout() {
        let s = build_script("const r = await Promise.resolve({n:7}); r", true, false);
        assert!(
            s.starts_with("(async function(){return ("),
            "the await wrap must stay outermost: {s}"
        );
        assert!(
            s.contains("(await (async function(){\nconst r = await Promise.resolve({n:7});\nreturn (\nr\n);})())"),
            "the inner statement IIFE must be async and awaited: {s}"
        );
        // No synchronous `function(){` may enclose the user's `await`: the
        // only non-async function in the output is the stringify helper
        // itself, which takes `v` as a parameter.
        assert!(
            !s.contains("(function(){"),
            "a synchronous IIFE containing await is a SyntaxError: {s}"
        );
        assert!(!s.contains("eval("), "must not contain eval(): {s}");
    }

    // ── iter-132 Theme C: top-level await ────────────────────────────────────

    #[test]
    fn contains_await_keyword_matches_whole_word_only() {
        assert!(contains_await_keyword("await Promise.resolve(1)"));
        assert!(contains_await_keyword("1 + await foo()"));
        assert!(contains_await_keyword("(await foo())"));
        // False positives that must NOT match (substring, not the keyword).
        assert!(!contains_await_keyword("awaited"));
        assert!(!contains_await_keyword("obj.awaitSomething()"));
        assert!(!contains_await_keyword("document.title"));
        assert!(!contains_await_keyword(""));
    }

    #[test]
    fn looks_like_single_expression_accepts_bare_expressions() {
        assert!(looks_like_single_expression(
            "await Promise.resolve(41) + 1"
        ));
        assert!(looks_like_single_expression("document.title"));
        assert!(looks_like_single_expression(
            "await fetch('/x').then(r => r.json())"
        ));
        // A single trailing semicolon is tolerated.
        assert!(looks_like_single_expression("await foo();"));
    }

    #[test]
    fn looks_like_single_expression_rejects_statement_lists() {
        assert!(!looks_like_single_expression("let x = await foo(); x + 1"));
        assert!(!looks_like_single_expression("const x = 1; x"));
        assert!(!looks_like_single_expression("if (true) { 1 } else { 2 }"));
        assert!(!looks_like_single_expression("return 1"));
        assert!(!looks_like_single_expression(""));
        assert!(!looks_like_single_expression("   "));
    }

    /// Regression: bare-word keyword entries in `STATEMENT_LEADING_KEYWORDS`
    /// (`"try"`, `"return"`, `"function"`) must match at a word boundary,
    /// not as a plain substring prefix — otherwise identifiers that merely
    /// start with those letters (`returnValue()`, `tryCatchWrapper()`,
    /// `functionCall()`) are misclassified as statements and silently lose
    /// the auto-return optimization even though they are ordinary single
    /// expressions.
    #[test]
    fn looks_like_single_expression_does_not_false_positive_on_keyword_prefixed_identifiers() {
        assert!(looks_like_single_expression("returnValue()"));
        assert!(looks_like_single_expression("tryCatchWrapper()"));
        assert!(looks_like_single_expression("functionCall()"));
        assert!(looks_like_single_expression("try_something()"));
        // The real keyword forms (word-boundary match) must still be
        // rejected as statements.
        assert!(!looks_like_single_expression("try { 1 } catch (e) { 2 }"));
        assert!(!looks_like_single_expression("function foo() { return 1 }"));
    }

    /// AC `live_132_eval_top_level_await` (unit half): a bare top-level
    /// `await` expression must be wrapped in an async IIFE with an explicit
    /// `return`, and must still never contain a bare `eval(` call (CSP
    /// invariant from iter-93 holds for the new wrap path too).
    #[test]
    fn build_script_wraps_top_level_await_single_expression() {
        let s = build_script("await Promise.resolve(41) + 1", false, false);
        assert!(
            s.starts_with("(async function(){return ("),
            "expected async-IIFE return wrap, got: {s}"
        );
        assert!(s.contains("await Promise.resolve(41) + 1"));
        assert!(!s.contains("eval("), "must not contain eval(): {s}");
    }

    /// iter-142 Theme E: a multi-statement script with top-level `await`
    /// whose *last* statement is a bare expression now has that trailing
    /// expression auto-returned — matching the non-await path's native
    /// eval-completion-value semantics — instead of silently discarding it
    /// as `undefined` (the "worst failure mode" flagged in the iteration
    /// plan). The earlier statement (`let x = await foo();`) still runs
    /// unwrapped, ahead of the synthesized `return`.
    #[test]
    fn build_script_wraps_top_level_await_multi_statement_honors_trailing_expression() {
        let s = build_script("let x = await foo(); x + 1", false, false);
        assert!(
            s.starts_with("(async function(){"),
            "expected async-IIFE wrap, got: {s}"
        );
        assert!(
            s.contains("let x = await foo();"),
            "earlier statement must still run: {s}"
        );
        assert!(
            s.contains("return (\nx + 1\n)"),
            "trailing bare expression must be auto-returned: {s}"
        );
        assert!(!s.contains("eval("), "must not contain eval(): {s}");
    }

    /// When the last statement is NOT a bare expression (here: a
    /// declaration with no completion value of its own), there is nothing
    /// safe to auto-return — the wrap must fall back to the no-auto-return
    /// form rather than guessing.
    #[test]
    fn build_script_wraps_top_level_await_multi_statement_no_auto_return_for_declaration_tail() {
        let s = build_script("await foo(); let x = 1", false, false);
        assert!(
            s.starts_with("(async function(){"),
            "expected async-IIFE wrap, got: {s}"
        );
        assert!(
            !s.contains("return ("),
            "a trailing declaration has no completion value to auto-return: {s}"
        );
        assert!(s.contains("await foo(); let x = 1"));
        assert!(!s.contains("eval("), "must not contain eval(): {s}");
    }

    /// AC `e2e_eval_asi_await_script` (unit half — the exact dogfooding
    /// session 63 repro): an ASI-separated (no `;` at all) two-line script
    /// must not leak the async-IIFE wrapper as a syntax error pointing past
    /// the end of the user's input, AND the trailing expression's value
    /// must be honored (not silently `undefined`) — both symptoms shared
    /// the same root cause (see `top_level_statement_boundaries`'s doc
    /// comment).
    #[test]
    fn build_script_asi_separated_await_script_wraps_without_leaking_and_returns_tail() {
        let s = build_script("await Promise.resolve(1)\n42", false, false);
        assert!(
            s.starts_with("(async function(){"),
            "expected async-IIFE wrap, got: {s}"
        );
        assert!(
            s.contains("await Promise.resolve(1)"),
            "the await statement must still run: {s}"
        );
        assert!(
            s.contains("return (\n42\n)"),
            "the ASI-separated trailing expression must be auto-returned: {s}"
        );
        // The old bug wrapped the whole two-line body as a single
        // `return (…)` expression, which is itself invalid JS — assert the
        // await statement is NOT inside the returned parenthetical.
        assert!(
            !s.contains("return (\nawait Promise.resolve(1)\n42\n)"),
            "must not reproduce the pre-fix leaky wrap: {s}"
        );
        assert!(!s.contains("eval("), "must not contain eval(): {s}");
    }

    /// `looks_like_multi_statement` (via `looks_like_single_expression`)
    /// must recognize ASI-separated statements even with zero `;`
    /// characters anywhere in the script.
    #[test]
    fn looks_like_single_expression_rejects_asi_separated_statements() {
        assert!(!looks_like_single_expression(
            "await Promise.resolve(1)\n42"
        ));
        assert!(!looks_like_single_expression("foo()\nbar()"));
    }

    /// A common legitimate multi-line style — a method chain whose
    /// continuation lines start with `.` — must still be recognized as a
    /// single expression, not misclassified as ASI-separated statements.
    #[test]
    fn looks_like_single_expression_accepts_multiline_method_chain() {
        assert!(looks_like_single_expression(
            "document\n  .querySelector('a')\n  .click()"
        ));
    }

    /// A multi-line object literal (newlines inside `{}`/`()` at non-zero
    /// bracket depth) must not be split into fake statements.
    #[test]
    fn looks_like_single_expression_accepts_multiline_object_literal() {
        assert!(looks_like_single_expression("({\n  a: 1,\n  b: 2\n})"));
    }

    /// Scripts without `await` must be completely unaffected by the new
    /// wrap logic (no behavior change for the common case).
    #[test]
    fn build_script_without_await_is_unaffected() {
        let s = build_script("document.title", false, false);
        assert_eq!(s, "document.title");
        assert!(!s.contains("async function"));
    }

    /// `--stringify` combined with a top-level-await expression: the async
    /// wrap must be the OUTERMOST layer (so evaluateJSAsync awaits the
    /// Promise before the stringify helper ever sees the value) — not
    /// nested inside the stringify IIFE, which would stringify the
    /// unresolved Promise object instead of its resolved value.
    #[test]
    fn build_script_stringify_with_await_wraps_outermost() {
        let s = build_script("await Promise.resolve(41)", true, false);
        assert!(
            s.starts_with("(async function(){return ("),
            "async wrap must be outermost, got: {s}"
        );
        assert!(s.contains("JSON.stringify("));
        assert!(s.contains("await Promise.resolve(41)"));
        assert!(!s.contains("eval("), "must not contain eval(): {s}");
    }
}
