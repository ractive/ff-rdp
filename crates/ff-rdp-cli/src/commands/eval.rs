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
/// iter-165 restores the promised contract by routing the plain path through
/// the same value-producing IIFE [`wrap_statements_in_iife`] that
/// `--stringify` (iter-161) and the top-level-`await` path (iter-132) already
/// used — those two paths were already isolated, so the plain synchronous
/// path was the sole exception.
///
/// The wrap is applied only when the script actually declares something at
/// top level ([`declares_at_top_level`]), not to every non-single-expression
/// script. A script that declares nothing cannot leak anything, so wrapping
/// it would buy no isolation while changing its completion value: a function
/// body has no script-completion-value semantics, so `eval 'if (1) { 2 }'`
/// would start returning `undefined` instead of `2`. Restricting the trigger
/// keeps every declaration-free script byte-for-byte on its pre-165 path.
///
/// iter-165 gave a second reason for the narrow trigger — it confined the
/// blast radius of [`top_level_statement_boundaries`], which did not
/// understand regex literals or comments. iter-167 Theme B fixed the scanner,
/// so that reason is gone; the trigger stays narrow on the completion-value
/// argument alone, which never depended on it. Measured against a live
/// Firefox at iter-167: converging the two triggers would cost
/// `eval 'if (1) { 2 }'` and `eval 'for (let i = 0; i < 3; i++) { i }'` their
/// completion values (`2` each, today) to make `eval 'return 1'` work — two
/// working behaviours traded for one, so they stay apart. See DEC-039.
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
    } else if isolate && !has_await && declares_at_top_level(user_script) {
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
/// Simplifications. Each is meant to fail *safe* — degrade to the
/// no-auto-return wrap path in [`wrap_top_level_await`], which still
/// evaluates, it just won't surface a value unless the script has an explicit
/// `return`. iter-170 measured that claim and found two of iter-167's
/// simplifications did NOT hold to it (see
/// [`top_level_statement_boundaries`]), so treat "fails safe" here as an
/// intent that needs checking against a live browser, not a guarantee:
///
/// - A `;`/newline inside a string, template literal, regex literal or
///   comment is tracked (see [`top_level_statement_boundaries`]) so those
///   don't false-positive as statement separators. The tracking is still
///   char-based rather than a real tokenizer — iter-167 closed the regex,
///   comment and backslash-escape gaps and iter-170 closed `${…}`
///   interpolation and the `}` ambiguity, but an arrow function's `{` body
///   and a `class` body are still classified as object literals, so a `/`
///   after either reads as division.
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
    // iter-167: `// note\nconst x = 1` is a declaration, not an expression —
    // the keyword check has to look past any leading comment to see that.
    let leading = trim_leading_trivia(body);
    !looks_like_multi_statement(body)
        && !leading.is_empty()
        && !STATEMENT_LEADING_KEYWORDS
            .iter()
            .any(|kw| body_starts_with_keyword(leading, kw))
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

/// Keywords after which a `/` can only begin a regular-expression literal,
/// never a division (iter-167).
///
/// [`slash_starts_regex`] decides regex-vs-division from the previous
/// significant character, and every one of these keywords ends in an
/// identifier character — so without this list `return /a;b/` would look
/// exactly like `count / 2` and the regex would be scanned as a division.
/// Keywords that are *statements* in their own right (`return`, `throw`,
/// `case`, `do`, `else`) and the unary operators (`typeof`, `void`,
/// `delete`, `new`, `await`, `yield`) are the whole set that can legally be
/// followed by a regex literal; `instanceof`/`in`/`of` are binary operators
/// whose right operand may also be one.
const KEYWORDS_BEFORE_REGEX: &[&str] = &[
    "return",
    "typeof",
    "instanceof",
    "in",
    "of",
    "new",
    "delete",
    "void",
    "throw",
    "case",
    "do",
    "else",
    "yield",
    "await",
];

/// Keywords that can only be followed by a *block* `{`, never by an object
/// literal (iter-170 Theme C).
///
/// These are the block-introducing keywords whose `{` is not already preceded
/// by a `)` (which [`brace_opens_block`] handles on its own): `do {}`,
/// `else {}`, `try {}`, `finally {}`. `return {a:1}` and `typeof {}` are
/// deliberately absent — those braces are object literals.
const KEYWORDS_BEFORE_BLOCK: &[&str] = &["do", "else", "try", "finally"];

/// Keywords that continue the construct a just-closed block belongs to, so the
/// `}` before them does NOT end a statement (iter-170 Theme C):
/// `if {} else {}`, `try {} catch {} finally {}`, `do {} while (x)`.
const KEYWORDS_CONTINUING_BLOCK: &[&str] = &["else", "catch", "finally", "while"];

/// What a `{` opened, as far as this scanner is willing to commit (iter-170
/// Theme C).
///
/// Only used to answer one question: does the `/` after the matching `}` start
/// a regular-expression literal ([`slash_starts_regex`])? `Unknown` is the
/// deliberate third state — an unbalanced `}`, or a `{` in a position this
/// scanner will not judge — and it keeps iter-167's answer (division), which
/// is the direction that only ever adds a spurious boundary.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum BraceKind {
    /// A statement block: `if (x) {}`, `{ … }`, a function body.
    Block,
    /// An object literal: `({a: 1})`, `f({})`, `x = {}`.
    ObjectLiteral,
    /// A `${…}` interpolation inside a template literal, whose `}` returns the
    /// scanner to template-literal state (iter-170 Theme B).
    Interpolation,
    /// Not judged — treated exactly as iter-167 treated every `}`.
    Unknown,
}

/// Whether the `{` whose preceding significant character is `prev_significant`
/// (at `prev_idx` in `chars`) opens a statement block rather than an object
/// literal (iter-170 Theme C).
///
/// Conservative by construction: it answers `true` only for positions where a
/// *statement* can start and an object literal cannot, and `false` for
/// everything else — so the fallback is iter-167's unconditional
/// "`}` divides", whose failure mode only ever adds a boundary.
///
/// - Nothing before it, or `;`, or `{` — a statement position; JS itself parses
///   a leading `{` as a block, not an object literal.
/// - `)` — `if (…) {`, `for (…) {`, `while (…) {`, `catch (…) {`,
///   `function f(…) {`. There is no valid JS in which an object literal
///   directly follows `)`.
/// - `}` — whatever the `{` it closed was: a block's `}` leaves a statement
///   position, an object literal's does not.
/// - `do` / `else` / `try` / `finally` ([`KEYWORDS_BEFORE_BLOCK`]), excluding a
///   dotted property access (`obj.try {`) for the same reason
///   [`slash_starts_regex`] excludes one.
///
/// - `=>` — an arrow function's body (iter-176 Theme B). `=>` is the only
///   two-character token ending in `>` whose first character is `=`, and the
///   `{` after one is always a block, never an object literal (`() => ({a:1})`
///   needs the parentheses precisely because of that).
/// - `class` / `class K` / `class K extends B` — a class body (iter-176 Theme
///   C). A class *expression*'s body never reaches here: the scanner marks it
///   at the `class` keyword and forces [`BraceKind::ObjectLiteral`], the same
///   way it does for a function expression.
/// - an identifier followed by `:` at statement position — a labelled block
///   (iter-176 Theme C, [`label_precedes_block`]).
fn brace_opens_block(
    chars: &[(usize, char)],
    prev_significant: Option<char>,
    prev_idx: Option<usize>,
    prev_brace: BraceKind,
) -> bool {
    let Some(prev) = prev_significant else {
        return true;
    };
    match prev {
        ';' | '{' | ')' => return true,
        '}' => return prev_brace == BraceKind::Block,
        // iter-176 Theme B: `=>`. No whitespace is allowed inside the token,
        // so the character immediately before the `>` decides it. `>=`, `>>`
        // and `>>>` all end in a character that is not `>`, or are preceded by
        // `>` rather than `=`, so none of them reaches this arm.
        '>' => return prev_idx.is_some_and(|i| i > 0 && chars[i - 1].1 == '='),
        // iter-176 Theme C: a labelled block, `outer: { … }`.
        ':' => return label_precedes_block(chars, prev_idx, prev_brace),
        _ => {}
    }
    let is_ident = |c: char| c.is_ascii_alphanumeric() || c == '_' || c == '$';
    if !is_ident(prev) {
        return false;
    }
    let Some(end) = prev_idx else {
        return false;
    };
    let mut start = end;
    while start > 0 && is_ident(chars[start - 1].1) {
        start -= 1;
    }
    if start > 0 && chars[start - 1].1 == '.' {
        return false;
    }
    let word: String = chars[start..=end].iter().map(|&(_, c)| c).collect();
    if KEYWORDS_BEFORE_BLOCK.contains(&word.as_str()) {
        return true;
    }
    // iter-176 Theme C: an anonymous class body, `class { … }`.
    if word == "class" {
        return true;
    }
    // iter-176 Theme C: `class K {` and `class K extends B {` put an
    // *identifier* before the `{`, so the keyword is one word further back.
    // Both `class` and `extends` are reserved words, so an identifier
    // preceded by either can only be a class name or a superclass — there is
    // no object literal in that position.
    let Some((prev_word_start, prev_word_end)) = word_before(chars, start) else {
        return false;
    };
    if prev_word_start > 0 && chars[prev_word_start - 1].1 == '.' {
        return false;
    }
    let prev_word: String = chars[prev_word_start..=prev_word_end]
        .iter()
        .map(|&(_, c)| c)
        .collect();
    matches!(prev_word.as_str(), "class" | "extends")
}

/// The `chars` range of the identifier-ish word ending immediately before
/// `at` (skipping whitespace), or `None` if there is no such word.
///
/// Split out in iter-176: [`brace_opens_block`] and [`label_precedes_block`]
/// both need to look one token further back than the character
/// `top_level_statement_boundaries` hands them.
fn word_before(chars: &[(usize, char)], at: usize) -> Option<(usize, usize)> {
    let is_ident = |c: char| c.is_ascii_alphanumeric() || c == '_' || c == '$';
    let mut k = at;
    while k > 0 && chars[k - 1].1.is_whitespace() {
        k -= 1;
    }
    if k == 0 || !is_ident(chars[k - 1].1) {
        return None;
    }
    let end = k - 1;
    let mut start = end;
    while start > 0 && is_ident(chars[start - 1].1) {
        start -= 1;
    }
    Some((start, end))
}

/// Whether the `:` at `colon_idx` is a *label* terminator — so the `{` after
/// it opens a labelled block, `outer: { break outer }` — rather than an object
/// literal key or a ternary's `:` (iter-176 Theme C).
///
/// iter-170 left this position unjudged because `{a: 1}` "looks the same from
/// the right", and read from the `:` alone it does. It stops looking the same
/// one token further left: the rule here is that the label identifier must sit
/// where a *statement* can start — nothing before it, a `;`, or a **block**'s
/// `}` — which is a position no object-literal key and no ternary branch can
/// occupy:
///
/// - `{a: {b:1}}`, `{a:1, b:{c:2}}` — the key is preceded by `{` or `,`.
/// - `c ? x : {a:1}` — the `:`-preceding identifier is preceded by `?`.
/// - `switch (x) { case 1: {…} }` — `1` is preceded by the word `case`.
///   (`default: {…}` *is* accepted, and is genuinely a block.)
///
/// A leading digit is rejected so a numeric object key (`{1: {a:2}}`) can
/// never be mistaken for a label even if it somehow reached a statement
/// position.
fn label_precedes_block(
    chars: &[(usize, char)],
    colon_idx: Option<usize>,
    prev_brace: BraceKind,
) -> bool {
    let Some(colon) = colon_idx else {
        return false;
    };
    let Some((start, _)) = word_before(chars, colon) else {
        return false;
    };
    if chars[start].1.is_ascii_digit() {
        return false;
    }
    let mut j = start;
    while j > 0 && chars[j - 1].1.is_whitespace() {
        j -= 1;
    }
    if j == 0 {
        return true;
    }
    match chars[j - 1].1 {
        ';' => true,
        '}' => prev_brace == BraceKind::Block,
        _ => false,
    }
}

/// Review fix (post-iter-170): whether a `function` keyword whose immediately
/// preceding significant character is `prev_significant` (at `prev_idx`) is a
/// *declaration* — i.e. [`brace_opens_block`] would call this a statement
/// position — rather than an *expression* like `const f = function(){}` or
/// `arr.map(function(x){})`.
///
/// A function expression's `{}` is still grammatically a block (not an object
/// literal — [`brace_opens_block`]'s `)`-preceded rule is right about that),
/// but unlike a declaration's block it does NOT end the enclosing *statement*:
/// `const f = function(){} / 2` is one statement (a division), and treating
/// its `}` as regex-permitting or self-terminating — which
/// [`top_level_statement_boundaries`] used to do for every `)`-preceded `{`,
/// declaration or not — turned this valid division into
/// `unterminated regular expression literal`. Measured live: `main` (this
/// branch pre-fix) throws on `const f = function(){} / 2`; `git show
/// origin/main` (pre-iter-170) and this fix both evaluate it (division,
/// `NaN`).
///
/// Skips back over a leading `async` so `async function foo(){}` at true
/// statement position is still recognized as a declaration.
fn function_keyword_is_declaration(
    chars: &[(usize, char)],
    prev_significant: Option<char>,
    prev_idx: Option<usize>,
    prev_brace: BraceKind,
) -> bool {
    let is_ident = |c: char| c.is_ascii_alphanumeric() || c == '_' || c == '$';
    if prev_significant == Some('c')
        && let Some(end) = prev_idx
    {
        let mut start = end;
        while start > 0 && is_ident(chars[start - 1].1) {
            start -= 1;
        }
        if chars[start..=end]
            .iter()
            .map(|&(_, c)| c)
            .eq("async".chars())
        {
            if start == 0 {
                return brace_opens_block(chars, None, None, prev_brace);
            }
            let mut k = start - 1;
            while chars[k].1.is_whitespace() {
                if k == 0 {
                    return brace_opens_block(chars, None, None, prev_brace);
                }
                k -= 1;
            }
            return brace_opens_block(chars, Some(chars[k].1), Some(k), prev_brace);
        }
    }
    brace_opens_block(chars, prev_significant, prev_idx, prev_brace)
}

/// Whether the `/` whose preceding significant character is `prev_significant`
/// (at `prev_idx` in `chars`) starts a regular-expression literal rather than
/// a division operator (iter-167 Theme B).
///
/// This is the standard lexer heuristic and needs exactly one character of
/// context, which [`top_level_statement_boundaries`] already carries: a `/`
/// is division only if the token before it can end an expression — a
/// closing bracket, a string terminator, or an identifier/number that is not
/// one of [`KEYWORDS_BEFORE_REGEX`]. At the very start of a script, or after
/// an operator/`(`/`,`/`;`/`=`, nothing is there to divide, so the `/` opens
/// a regex.
///
/// The `}` case needs one extra bit of context, supplied by `prev_brace`
/// (iter-170 Theme C): `}` ends both an object literal (`({a:1}) / 2` —
/// division) and a block (`if (x) {} /re/` — regex), and until iter-170 this
/// function called it division unconditionally. That was *not* fail-safe as
/// iter-167 assumed: measured on live Firefox, `const n = 1; if (n) {}
/// /a;b/.test("a;b")` scanned the regex as a division, reported the `;`
/// inside it as a top-level boundary, and the wrap emitted
/// `unterminated regular expression literal`. [`top_level_statement_boundaries`]
/// now records what each `{` opened ([`brace_opens_block`]) and passes the
/// kind of the most recently closed one here, so a block's `}` is followed by
/// a regex (which is also what the JS grammar says) and an object literal's
/// by a division. When the kind is unknown — an unbalanced `}`, or a `{`
/// whose position this scanner will not commit on — `prev_brace` is
/// [`BraceKind::Unknown`] and the old division answer stands.
///
/// A dotted property access (`obj.in`, `obj.new`, …) is excluded from the
/// keyword match even though the word matches: every reserved word is a
/// legal property name after a bare `.` since ES5, and misreading one as the
/// keyword is the one direction that does *not* fail safe (it can hide a
/// real boundary rather than add a spurious one).
fn slash_starts_regex(
    chars: &[(usize, char)],
    prev_significant: Option<char>,
    prev_idx: Option<usize>,
    prev_brace: BraceKind,
) -> bool {
    let Some(prev) = prev_significant else {
        // Nothing before it: a leading `/` cannot be a division.
        return true;
    };
    if !is_statement_end_char(prev) {
        return true;
    }
    if prev == '}' {
        // iter-170: a block's `}` ends a statement, so the `/` after it opens
        // a regex; an object literal's `}` ends an expression, so it divides.
        return prev_brace == BraceKind::Block;
    }
    let is_ident = |c: char| c.is_ascii_alphanumeric() || c == '_' || c == '$';
    if !is_ident(prev) {
        // `)`, `]` or a closing quote — all end an expression, so divide.
        return false;
    }
    let Some(end) = prev_idx else {
        return false;
    };
    let mut start = end;
    while start > 0 && is_ident(chars[start - 1].1) {
        start -= 1;
    }
    if start > 0 && chars[start - 1].1 == '.' {
        // `obj.in`, `obj.new`, `obj.case`, … — every reserved word is a
        // legal property name after a bare `.` since ES5 (iter-167 review
        // fix). Without this check `obj.in / 2` misread `in` as the
        // keyword, judged the `/` a regex open, and the resulting
        // (wrong-direction) scan could swallow a real top-level `;` that
        // followed — the one case in this heuristic that does NOT fail
        // safe, since it hides a boundary instead of only adding one.
        return false;
    }
    let word: String = chars[start..=end].iter().map(|&(_, c)| c).collect();
    KEYWORDS_BEFORE_REGEX.contains(&word.as_str())
}

/// Index in `chars` of the `/` that closes the regular-expression literal
/// opening at `start`, or `None` if there is no valid one (iter-167).
///
/// Handles backslash escapes (`/a\/b/`) and character classes (`/[/]/`, where
/// the `/` is literal). A regex literal may not contain an unescaped line
/// terminator, so a newline means this was not a regex after all and the
/// caller must fall back to treating the `/` as division — which is the
/// fail-safe direction: the scan is abandoned rather than swallowing the rest
/// of the script.
fn scan_regex_literal(chars: &[(usize, char)], start: usize) -> Option<usize> {
    let mut i = start + 1;
    let mut in_class = false;
    while i < chars.len() {
        match chars[i].1 {
            '\\' => i += 1,
            '\n' => return None,
            '[' => in_class = true,
            ']' => in_class = false,
            '/' if !in_class => return Some(i),
            _ => {}
        }
        i += 1;
    }
    None
}

/// The byte offset of the first non-whitespace character at or after `from`,
/// if the break before it is an Automatic Semicolon Insertion boundary —
/// [`is_statement_end_char`] before it, NOT [`is_continuation_start_char`]
/// after.
///
/// Split out of [`top_level_statement_boundaries`] in iter-167 because a
/// block comment containing a line terminator is an ASI boundary candidate in
/// exactly the same way the newline it hides would have been, so the same
/// check now runs from two places.
fn asi_boundary_after(
    chars: &[(usize, char)],
    from: usize,
    prev_significant: Option<char>,
) -> Option<usize> {
    let mut j = from;
    while j < chars.len() && chars[j].1.is_whitespace() {
        j += 1;
    }
    let prev = prev_significant?;
    let &(next_byte, next) = chars.get(j)?;
    (is_statement_end_char(prev) && !is_continuation_start_char(next)).then_some(next_byte)
}

/// The byte offset of the statement that starts right after a top-level
/// block's closing `}` at `from`, if there is one (iter-170 Theme C).
///
/// A block statement is self-terminating: `if (n) {} expr` is two statements
/// with no `;` and no newline between them, and until iter-170 the scanner had
/// no way to know, because it could not tell a block's `}` from an object
/// literal's. Now that [`brace_opens_block`] classifies them, the boundary is
/// derivable — which is what makes the dogfood line
/// `const n = 1; if (n) {} /a;b/.test("a;b")` return `true` instead of
/// `undefined`.
///
/// Suppressed, in the fail-safe direction (no boundary, i.e. exactly the
/// pre-170 answer), when what follows cannot start a statement:
///
/// - an [`is_continuation_start_char`] — `,`, `;`, `.`, an operator, a closing
///   bracket. `/` is the one exception: after a *block*'s `}` a `/` opens a
///   regex literal, which is a new statement (that is the whole point of
///   [`slash_starts_regex`]'s `}` case) — unless it opens a comment, which is
///   trivia and whose trailing newline the ASI arm handles.
/// - `(`, `[` or a backtick — a call, an index or a tagged template applied to
///   whatever preceded, e.g. the `!function(){}()` IIFE form.
/// - a [`KEYWORDS_CONTINUING_BLOCK`] keyword — `else`, `catch`, `finally`,
///   `while`.
fn block_boundary_after(chars: &[(usize, char)], from: usize) -> Option<usize> {
    let mut j = from;
    while j < chars.len() && chars[j].1.is_whitespace() {
        j += 1;
    }
    let &(byte, next) = chars.get(j)?;
    let is_comment = next == '/' && matches!(chars.get(j + 1).map(|&(_, c)| c), Some('/' | '*'));
    if is_comment || matches!(next, '(' | '[' | '`') {
        return None;
    }
    if next != '/' && is_continuation_start_char(next) {
        return None;
    }
    let is_ident = |c: char| c.is_ascii_alphanumeric() || c == '_' || c == '$';
    let mut k = j;
    while k < chars.len() && is_ident(chars[k].1) {
        k += 1;
    }
    let word: String = chars[j..k].iter().map(|&(_, c)| c).collect();
    (!KEYWORDS_CONTINUING_BLOCK.contains(&word.as_str())).then_some(byte)
}

/// Append `at` to `boundaries` unless it is already the last entry.
///
/// iter-167: two paths can now propose the same offset — a block comment's
/// ASI check ([`asi_boundary_after`]) and the check for a later newline — and
/// a repeated boundary would make [`wrap_statements_in_iife`] split on an
/// empty statement.
fn push_boundary(boundaries: &mut Vec<usize>, at: usize) {
    if boundaries.last() != Some(&at) {
        boundaries.push(at);
    }
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
///
/// # iter-167: regex literals, comments and escapes
///
/// Until iter-167 the scanner tracked *only* quotes and depth, and iter-165
/// narrowed its own wrap trigger to work around that. Measured on main
/// (2026-08-16, `kb/iterations/iteration-167-*`), five inputs emitted invalid
/// JavaScript, e.g. `eval --stringify '/a;b/.test("a;b")'` →
/// `unterminated regular expression literal`, because the `;` inside the
/// regex was reported as a top-level boundary and the wrap split the script
/// into `/a;` and `b/.test("a;b")`. Now also tracked:
///
/// - **Regex literals**, opened by a `/` that [`slash_starts_regex`] judges
///   cannot be a division and closed by [`scan_regex_literal`].
/// - **`//` line comments**, skipped up to but *not including* their
///   terminating newline, which is still an ASI boundary candidate; and
///   **`/* */` block comments**, skipped whole, with an
///   [`asi_boundary_after`] check when the comment spans a line terminator.
///   Before this, an apostrophe in a comment (`// don't`) opened string state
///   and swallowed the rest of the script.
/// - **Backslash escapes** inside single-quoted, double-quoted and template
///   strings as well as regex literals, so `"a\";b"` no longer ends its
///   string at the escaped quote.
///
/// # iter-170: `${…}` interpolation and the `}` ambiguity
///
/// iter-167 left two gaps open and called both fail-safe. Measured on live
/// Firefox (2026-08-17, `kb/iterations/iteration-170-*`) neither was:
///
/// - **`${…}` interpolation** was skipped as opaque template text, so
///   `` const s = `a${"`"}b`; s `` closed its template at the interpolated
///   backtick, opened double-quote state on the `"` after it, and swallowed
///   the script's real top-level `;`. With no boundary to split on, the
///   iter-165 wrap emitted an IIFE with no `return` and the value silently
///   became `undefined`. The interpolation is now *re-entered*: `${` pushes a
///   [`BraceKind::Interpolation`] brace and restores full code state (strings,
///   regex literals, comments, nesting), and its matching `}` returns the
///   scanner to the template.
/// - **A `/` after `}`** was always read as division, so
///   `const n = 1; if (n) {} /a;b/.test("a;b")` reported the `;` *inside* the
///   regex as a top-level boundary and the wrap emitted
///   `unterminated regular expression literal`. Each `{` is now classified by
///   [`brace_opens_block`] and the kind of the most recently closed brace is
///   handed to [`slash_starts_regex`].
///
/// Classifying braces also makes a third boundary derivable for the first
/// time: a block statement terminates itself, so the token after a top-level
/// block's `}` starts a new statement with no `;` and no newline between them
/// ([`block_boundary_after`]). Without it the gap-2 script above stopped
/// being a SyntaxError but returned `undefined` — the failure iter-142 Theme E
/// named the worst of this wrap — because `if (n) {} /a;b/.test("a;b")` was
/// still one statement beginning with `if`, and an `if` is not something the
/// wrap can auto-return.
///
/// iter-176 closed the three positions iter-170 left unjudged, after measuring
/// that all three turn valid JavaScript into a SyntaxError (and, for a class
/// declaration, into a silent `undefined`) on a live browser:
/// [`brace_opens_block`] now commits on an arrow function's `{` body, a class
/// body and a labelled block. See
/// `kb/iterations/iteration-176-eval-scanner-brace-positions.md`.
///
/// Still not a JS tokenizer, and deliberately so (all code stays in Rust and
/// this repo has no JS parser dependency). Known remaining gaps:
///
/// - An object literal, ternary branch or `case` label nested inside a
///   non-block brace keeps the pre-170 answer, because
///   [`label_precedes_block`] only commits at a statement position it can see
///   from one token of lookback. Fail-safe: a missing boundary, never a
///   spurious one.
/// - `const g = () => {} /re/.test(s)` — with no line terminator, Firefox
///   rejects this (an ArrowFunction is not a division operand and ASI needs a
///   newline) but the scanner accepts it, because it reads the arrow body's
///   `}` as self-terminating the way a block statement's is. A deliberate
///   trade: the same rule is what makes the newline form — which *is* valid
///   JavaScript — work. The divergence only ever accepts input Firefox would
///   reject; it never changes the value of a valid script.
/// - The stale-marker residual documented on
///   [`function_keyword_is_declaration`] now also applies to a `class`
///   keyword in expression position that is never followed by a `{` at the
///   same depth. Unreached by any live or unit input, and in the safe
///   direction.
fn top_level_statement_boundaries(body: &str) -> Vec<usize> {
    #[derive(Clone, Copy, PartialEq)]
    enum Str {
        None,
        Single,
        Double,
        Template,
    }

    let mut boundaries: Vec<usize> = Vec::new();
    let mut state = Str::None;
    let mut depth: i32 = 0;
    let mut prev_significant: Option<char> = None;
    let mut prev_significant_idx: Option<usize> = None;
    // iter-170: what each currently-open `{` opened, and what the most
    // recently *closed* one was. Only ever consulted while
    // `prev_significant` is `}`, so the last-closed brace is that `}`.
    let mut braces: Vec<BraceKind> = Vec::new();
    let mut prev_brace = BraceKind::Unknown;
    // Review fix (post-iter-170), extended to `class` in iter-176: depths at
    // which a `function` or `class` keyword was seen in *expression* position
    // (`function_keyword_is_declaration` / `brace_opens_block` false).
    // Consulted only at the `{` that follows — if that `{` is reached at the
    // same depth, it is this function's or class's body, and must not be
    // classified `Block` the way a declaration's body is (see
    // `function_keyword_is_declaration`'s doc comment for the regression this
    // closes; `const C = class {} / 2` is the class analogue, and unlike the
    // arrow case it really is a valid division because a ClassExpression is a
    // PrimaryExpression).
    let mut expr_body_depths: Vec<i32> = Vec::new();
    let is_ident_char = |c: char| c.is_ascii_alphanumeric() || c == '_' || c == '$';
    let chars: Vec<(usize, char)> = body.char_indices().collect();
    let n = chars.len();
    let mut i = 0;

    while i < n {
        let (byte_idx, c) = chars[i];

        // --- inside a string/template literal ---------------------------------
        if state != Str::None {
            // iter-167: a backslash escapes the next character, so `"a\";b"`
            // stays one string instead of ending at the escaped quote.
            if c == '\\' {
                i += 2;
                continue;
            }
            // iter-170: `${` inside a template literal opens an ordinary code
            // context, not more template text. Re-enter it with full state;
            // the `}` arm below restores template state when this brace pops.
            if state == Str::Template && c == '$' && chars.get(i + 1).map(|&(_, c)| c) == Some('{')
            {
                braces.push(BraceKind::Interpolation);
                depth += 1;
                state = Str::None;
                // An interpolation holds an *expression*, never a statement,
                // so `(` is the right stand-in: `slash_starts_regex` reads a
                // `/` after it as a regex (`${/re/.test(s)}`) and
                // `brace_opens_block` reads a `{` after it as an object
                // literal (`${ {a:1}.a }`), both of which are correct here.
                prev_significant = Some('(');
                prev_significant_idx = Some(i + 1);
                i += 2;
                continue;
            }
            let closer = match state {
                Str::Single => '\'',
                Str::Double => '"',
                _ => '`',
            };
            if c == closer {
                state = Str::None;
                prev_significant = Some(c);
                prev_significant_idx = Some(i);
            }
            i += 1;
            continue;
        }

        // --- comments and regex literals (iter-167) ---------------------------
        if c == '/' {
            let next = chars.get(i + 1).map(|&(_, c)| c);
            if next == Some('/') {
                // Stop *at* the newline so the ASI arm below still sees it.
                let mut j = i + 2;
                while j < n && chars[j].1 != '\n' {
                    j += 1;
                }
                i = j;
                continue;
            }
            if next == Some('*') {
                let mut j = i + 2;
                let mut spans_line = false;
                let mut closed = false;
                while j < n {
                    if chars[j].1 == '\n' {
                        spans_line = true;
                    }
                    if chars[j].1 == '*' && chars.get(j + 1).map(|&(_, c)| c) == Some('/') {
                        closed = true;
                        break;
                    }
                    j += 1;
                }
                let after = if closed { j + 2 } else { n };
                if spans_line
                    && depth == 0
                    && let Some(at) = asi_boundary_after(&chars, after, prev_significant)
                {
                    push_boundary(&mut boundaries, at);
                }
                i = after;
                continue;
            }
            if slash_starts_regex(&chars, prev_significant, prev_significant_idx, prev_brace)
                && let Some(end) = scan_regex_literal(&chars, i)
            {
                // A regex literal is a complete primary expression, like a
                // string literal: what follows it may be a division and a
                // newline after it is an ASI boundary. `)` is the character
                // that already carries both of those properties through
                // `is_statement_end_char`/`slash_starts_regex`.
                prev_significant = Some(')');
                prev_significant_idx = Some(end);
                i = end + 1;
                continue;
            }
            // Otherwise it is a division operator; fall through.
        }

        // Review fix (post-iter-170): recognize a `function` keyword at a
        // fresh word boundary — but not a `.function` property/method access,
        // which is never the keyword — so its body's `{` can be told apart
        // from a declaration's at the point that `{` is reached. See
        // `function_keyword_is_declaration`.
        if state == Str::None
            && c == 'f'
            && (i == 0 || !is_ident_char(chars[i - 1].1))
            && prev_significant != Some('.')
            && chars
                .get(i..i + 8)
                .is_some_and(|w| w.iter().map(|&(_, ch)| ch).eq("function".chars()))
            && chars.get(i + 8).is_none_or(|&(_, ch)| !is_ident_char(ch))
            && !function_keyword_is_declaration(
                &chars,
                prev_significant,
                prev_significant_idx,
                prev_brace,
            )
        {
            expr_body_depths.push(depth);
        }

        // iter-176 Theme C: the same treatment for a `class` keyword in
        // expression position (`const C = class {}`, `f(class {})`), whose
        // body's `}` — unlike a class *declaration*'s — does not end a
        // statement and may legally be divided.
        if state == Str::None
            && c == 'c'
            && (i == 0 || !is_ident_char(chars[i - 1].1))
            && prev_significant != Some('.')
            && chars
                .get(i..i + 5)
                .is_some_and(|w| w.iter().map(|&(_, ch)| ch).eq("class".chars()))
            && chars.get(i + 5).is_none_or(|&(_, ch)| !is_ident_char(ch))
            && !brace_opens_block(&chars, prev_significant, prev_significant_idx, prev_brace)
        {
            expr_body_depths.push(depth);
        }

        match c {
            '\'' => state = Str::Single,
            '"' => state = Str::Double,
            '`' => state = Str::Template,
            '(' | '[' => depth += 1,
            '{' => {
                // Review fix (post-iter-170), extended to `class` in
                // iter-176: a function or class *expression*'s body, reached
                // at the same depth its keyword was seen at. Force
                // `ObjectLiteral` — the pre-170 safe answer — no matter what
                // `brace_opens_block` would say from the `)` or identifier
                // immediately before this `{`, neither of which can by itself
                // tell a declaration from an expression.
                let is_expr_body = expr_body_depths.last() == Some(&depth);
                if is_expr_body {
                    expr_body_depths.pop();
                }
                depth += 1;
                braces.push(
                    if !is_expr_body
                        && brace_opens_block(
                            &chars,
                            prev_significant,
                            prev_significant_idx,
                            prev_brace,
                        )
                    {
                        BraceKind::Block
                    } else {
                        BraceKind::ObjectLiteral
                    },
                );
            }
            ')' | ']' => depth = depth.saturating_sub(1),
            '}' => {
                depth = depth.saturating_sub(1);
                prev_brace = braces.pop().unwrap_or(BraceKind::Unknown);
                if prev_brace == BraceKind::Interpolation {
                    // Back inside the template literal this `${` interrupted.
                    // `prev_significant` is deliberately left alone: the next
                    // significant character is the template's own closing
                    // backtick, which the string arm records.
                    state = Str::Template;
                    i += 1;
                    continue;
                }
                // iter-170: a block statement terminates itself — no `;` and no
                // newline are needed after it.
                if prev_brace == BraceKind::Block
                    && depth == 0
                    && let Some(at) = block_boundary_after(&chars, i + 1)
                {
                    push_boundary(&mut boundaries, at);
                }
            }
            ';' if depth == 0 => push_boundary(&mut boundaries, byte_idx + c.len_utf8()),
            '\n' if depth == 0 => {
                if let Some(at) = asi_boundary_after(&chars, i + 1, prev_significant) {
                    push_boundary(&mut boundaries, at);
                }
            }
            _ => {}
        }

        // iter-167: tracked at every depth, not just depth 0. The scanner now
        // consults `prev_significant` to tell a regex from a division, and a
        // regex inside an argument list (`foo(/a'b/)`) used to open string
        // state on its apostrophe and swallow the rest of the script.
        if state == Str::None && !c.is_whitespace() {
            prev_significant = Some(c);
            prev_significant_idx = Some(i);
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

/// Declaration forms that install a binding in the *target global* when the
/// script is evaluated at top level: `const`/`let`/`class` go into the global
/// lexical environment, `var`/`function` onto the global object. These are
/// exactly the forms that survived from one `ff-rdp eval` to the next before
/// iter-165.
///
/// A declaration nested inside a block, loop head or function body is already
/// scoped to that construct and cannot leak, which is why the check below
/// only looks at the start of each *top-level* statement.
const DECLARATION_LEADING_KEYWORDS: &[&str] = &["var ", "let ", "const ", "class ", "function"];

/// Whether `script` declares something at top level that would otherwise be
/// installed in the target global (see [`DECLARATION_LEADING_KEYWORDS`]).
///
/// This is the iter-165 wrap trigger. It reuses
/// [`top_level_statement_boundaries`] to find where each top-level statement
/// begins and matches [`body_starts_with_keyword`] against each — so
/// `const x = 1; x` and `foo(); let y = 2; y` both qualify, while
/// `if (1) { const z = 2 }` (block-scoped, cannot leak) and
/// `for (const a of xs) {}` (loop-scoped) do not.
///
/// Best-effort in the same way as everything else built on that scanner, and
/// it fails *safe*: a missed declaration leaves the script on its pre-165
/// path (it leaks, as it did before), and a spurious hit only costs the wrap,
/// whose completion-value rule is documented in `eval --help`.
fn declares_at_top_level(script: &str) -> bool {
    let body = script.trim();
    if body.is_empty() {
        return false;
    }
    // Every boundary is a byte index in `body` at or before its end (the
    // trailing `;` of `const x = 1;` yields exactly `body.len()`), so the
    // slice below is always in range.
    std::iter::once(0)
        .chain(top_level_statement_boundaries(body))
        .any(|start| {
            // iter-167: a statement may begin with a comment.
            let statement = trim_leading_trivia(&body[start..]);
            DECLARATION_LEADING_KEYWORDS
                .iter()
                .any(|kw| body_starts_with_keyword(statement, kw))
        })
}

/// `s` with leading whitespace and leading `//` / `/* */` comments removed
/// (iter-167).
///
/// [`top_level_statement_boundaries`] reports where a statement *begins*, and
/// a statement may begin with trivia: in `// note\nconst x = 1; x` the first
/// statement's text starts with the comment, so matching
/// [`DECLARATION_LEADING_KEYWORDS`] or [`STATEMENT_LEADING_KEYWORDS`] against
/// it raw sees `//` and concludes the script neither declares anything nor is
/// a statement — which sent a commented declaration down the wrong path.
/// An unterminated comment consumes the rest of the input, which is what the
/// JS grammar does with it too.
fn trim_leading_trivia(s: &str) -> &str {
    let mut rest = s.trim_start();
    loop {
        rest = if let Some(after) = rest.strip_prefix("//") {
            after.find('\n').map_or("", |k| &after[k + 1..])
        } else if let Some(after) = rest.strip_prefix("/*") {
            after.find("*/").map_or("", |k| &after[k + 2..])
        } else {
            return rest;
        }
        .trim_start();
    }
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
    // iter-165: `--no-isolate` is honoured again. By default a script that
    // declares something at top level runs inside a per-call IIFE, so its
    // `const`/`let`/`class` never leak into the next `eval`; `--no-isolate`
    // sends it to the shared global lexical environment instead. See
    // [`build_script`]'s doc comment.
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

    /// The iter-165 wrap trigger. Only a *top-level* declaration can escape
    /// into the target global; anything else keeps its pre-165 path.
    #[test]
    fn unit_165_declares_at_top_level_matches_only_leaking_forms() {
        for script in [
            "const x = 1; x",
            "let y = 1; y",
            "var v = 1; v",
            "class C {}; 1",
            "function f(){return 1}; f()",
            "foo(); const later = 1; later",
            "const only = 1",
            "const asi = 1\nasi",
        ] {
            assert!(
                declares_at_top_level(script),
                "{script:?} declares at top level and must trigger the wrap"
            );
        }
        for script in [
            "document.title",
            "1 + 1",
            "throw new Error('boom')",
            "if (1) { 2 }",
            // Block- and loop-scoped declarations cannot reach the global.
            "if (1) { const z = 2 }",
            "for (const a of [1]) { a }",
            "(function(){ const inner = 1; return inner })()",
            // Identifiers that merely start with a keyword's letters.
            "constant()",
            "letters.length",
            "functionCall()",
            "",
            "   ",
        ] {
            assert!(
                !declares_at_top_level(script),
                "{script:?} declares nothing at top level and must stay on its \
                 pre-165 path"
            );
        }
    }

    /// A declaration-free script must come out of `build_script` byte-for-byte
    /// unchanged, whatever its statement shape. iter-165 wrote this as the
    /// guard on its wrap's blast radius, because
    /// [`top_level_statement_boundaries`] did not understand regex literals or
    /// comments; iter-167 fixed the scanner, so the test now pins the
    /// completion-value contract instead — these scripts must keep reaching
    /// `Debugger.evalInGlobal` as themselves.
    #[test]
    fn unit_165_declaration_free_scripts_are_never_rewritten() {
        for script in [
            "if (1) { 2 }",
            "for (let i = 0; i < 3; i++) { i }",
            "document.title\n// trailing comment",
            "/a;b/.test('a;b')",
            "throw new Error('boom')",
        ] {
            assert_eq!(
                build_script(script, false, true),
                script,
                "{script:?} declares nothing and must be sent verbatim"
            );
        }
    }

    // ── iter-165: per-call scope on the plain synchronous path ───────────────

    /// The defect this iteration fixes. Before iter-165 the plain path sent
    /// `const x = 1; x` to `Debugger.evalInGlobal` verbatim, so the binding
    /// landed in the tab's global lexical environment and the *second*
    /// identical call died with `redeclaration of const x`. The IIFE wrap
    /// keeps the binding call-local, and the trailing bare expression is
    /// still auto-returned so the value is unchanged.
    #[test]
    fn unit_165_plain_declaring_script_is_wrapped_per_call() {
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

        // Claim: a script that declares nothing is sent verbatim.
        assert!(
            help.contains(
                "A script that declares nothing cannot leak anything, so it is sent verbatim"
            ),
            "help must document the declaration-free passthrough: {help}"
        );
        assert_eq!(
            build_script("document.title", false, true),
            "document.title"
        );
        assert_eq!(build_script("if (1) { 2 }", false, true), "if (1) { 2 }");

        // Claim: the wrap trigger is a top-level declaration, and the help
        // names every form it covers.
        for kw in ["`const`", "`let`", "`class`", "`var`", "`function`"] {
            assert!(
                help.contains(kw),
                "help must name {kw} as a declaration form the wrap covers: {help}"
            );
        }
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
    ///
    /// iter-167 appended the five inputs whose generated script did not parse
    /// on main (the Theme A table in `kb/iterations/iteration-167-*`). Each
    /// declaring entry uses a distinct binding name on purpose: exactly one
    /// combination per script (`stringify=false, isolate=false`) is sent to
    /// Firefox verbatim and therefore declares into the tab's real global, so
    /// two scripts sharing a name would collide with `redeclaration of const`
    /// and blame the wrap for it.
    const MATRIX_SCRIPTS: [&str; 9] = [
        "document.title",
        "1 + 1",
        "const x = 1; x",
        "throw new Error('boom')",
        r#"/a;b/.test("a;b")"#,
        r#"const s167a = "x"; /a;b/.test("a;b")"#,
        r#"const s167b = "a\";b"; s167b"#,
        "const s167c = `a\\`;b`; s167c",
        "// don't touch\nconst s167d = 1; s167d",
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

    // ── iter-167: the scanner understands regex literals, comments, escapes ──

    /// AC: `unit_167_scanner_ignores_regex_and_comments`.
    ///
    /// The headline defect. Measured on main against a live Firefox
    /// (2026-08-16): `eval --stringify '/a;b/.test("a;b")'` returned
    /// `unterminated regular expression literal`, because the `;` inside the
    /// regex was reported as a top-level statement boundary and the wrap split
    /// the script into `/a;` and `b/.test("a;b")`. The same class of misfire
    /// hid inside `//` and `/* */` comments.
    #[test]
    fn unit_167_scanner_ignores_regex_and_comments() {
        for script in [
            r#"/a;b/.test("a;b")"#,
            "const r = /a;b/",
            "// a; b",
            "/* a; b */",
            "const x = 1 /* a;\nb */",
            "x.replace(/;/g, ',')",
            // A `;` inside a character class, and an escaped `/` inside the
            // literal, must not end the regex early either.
            r"/[;/]/.test(';')",
            r"/a\/b;c/.test('a/b;c')",
        ] {
            assert!(
                top_level_statement_boundaries(script).is_empty(),
                "{script:?} is one statement — the `;` is inside a regex or \
                 comment, got {:?}",
                top_level_statement_boundaries(script)
            );
        }

        // …and a real top-level `;` is still a boundary, including one that
        // follows a regex literal or a comment.
        for script in [
            "const r = /a;b/; r.test('a;b')",
            "const x = 1; // a; b\nx",
            "const x = 1 /* c */; x",
            "a; b",
        ] {
            assert!(
                !top_level_statement_boundaries(script).is_empty(),
                "{script:?} has a real top-level `;` and must split"
            );
        }
    }

    /// Review fix (iter-167): every reserved word is a legal property name
    /// after a bare `.` since ES5, so `obj.in`, `obj.new`, `obj.case`, … are
    /// member access, not the keywords `slash_starts_regex` matches against.
    /// Before the fix, `obj.in / 2; foo() / 3` misread the first `/` as a
    /// regex open and its scan swallowed the real top-level `;` between the
    /// two slashes — the one direction of this heuristic that does not fail
    /// safe, since it hides a boundary instead of only adding one.
    #[test]
    fn unit_167_dotted_property_named_like_keyword_is_not_the_keyword() {
        let boundaries = top_level_statement_boundaries("obj.in / 2; foo() / 3");
        assert_eq!(
            boundaries,
            vec!["obj.in / 2;".len()],
            "expected the real `;` boundary, regex misdetection swallowed it: {boundaries:?}"
        );

        for script in [
            "obj.in / 2",
            "obj.new / 2",
            "obj.case / 2",
            "obj.instanceof / 2",
            "obj.typeof / 2",
        ] {
            assert!(
                looks_like_single_expression(script),
                "{script:?} is a single division expression, not a regex"
            );
        }
    }

    /// AC: `unit_167_division_is_not_a_regex`.
    ///
    /// The risk the fix introduces: teaching the scanner that `/` can open a
    /// regex must not make it swallow a division. Regex-vs-division is decided
    /// by the previous significant character — `a` can end an expression, so
    /// `a / b` is division; `return` cannot, so `return /re/` is a regex.
    #[test]
    fn unit_167_division_is_not_a_regex() {
        // `a / b; c / d` must still split at its real `;`. If the first `/`
        // were read as a regex it would run to the second `/`, swallowing the
        // `;` with it.
        let boundaries = top_level_statement_boundaries("a / b; c / d");
        assert_eq!(
            boundaries,
            vec!["a / b;".len()],
            "division must not be scanned as a regex literal"
        );

        for script in ["x / y", "(a + b) / 2", "arr[0] / 2", "'8' / 2", "8 / 2 / 2"] {
            assert!(
                top_level_statement_boundaries(script).is_empty(),
                "{script:?} is a single division expression, got {:?}",
                top_level_statement_boundaries(script)
            );
            assert!(
                looks_like_single_expression(script),
                "{script:?} must stay a single expression"
            );
        }

        // A keyword before the slash flips the decision back to regex: the
        // `;` inside these literals must not be reported as a boundary.
        for script in ["return /a;b/", "typeof /a;b/", "x = 1 instanceof /a;b/"] {
            assert!(
                top_level_statement_boundaries(script).is_empty(),
                "{script:?}: a `/` after a keyword opens a regex, got {:?}",
                top_level_statement_boundaries(script)
            );
        }
    }

    /// Backslash escapes inside strings, templates and regex literals.
    /// Measured on main: `eval --stringify 'const s = "a\";b"; s'` failed with
    /// `"" string literal contains an unescaped line break` — the scanner
    /// ended the string at the *escaped* quote, so the following `;` looked
    /// top-level. The template-literal form failed the same way with
    /// `expected expression, got ')'`.
    #[test]
    fn unit_167_backslash_escapes_do_not_end_a_literal() {
        for script in [
            r#"const s = "a\";b""#,
            r"const s = 'a\';b'",
            "const s = `a\\`;b`",
            r#"const s = "a\\""#,
        ] {
            assert!(
                top_level_statement_boundaries(script).is_empty(),
                "{script:?}: the `;`/quote is escaped inside the literal, got {:?}",
                top_level_statement_boundaries(script)
            );
        }

        // The wrap that used to emit invalid JS now emits a valid IIFE.
        let s = build_script(r#"const s = "a\";b"; s"#, true, false);
        assert!(
            s.contains(r#"const s = "a\";b";"#),
            "the whole declaration must stay in the prefix, got: {s}"
        );
        assert!(
            s.contains("return (\ns\n)"),
            "expected `s` auto-returned: {s}"
        );
    }

    /// An apostrophe inside a `//` comment used to open single-quote string
    /// state and swallow the rest of the script, so `declares_at_top_level`
    /// missed the `const` and `--stringify` produced
    /// `expected expression, got keyword 'const'` (measured on main).
    #[test]
    fn unit_167_comment_contents_are_not_scanned() {
        let script = "// don't touch\nconst x = 1; x";
        assert!(
            declares_at_top_level(script),
            "the `const` after the comment must still be seen"
        );
        let s = build_script(script, true, false);
        assert!(
            s.contains("const x = 1;"),
            "the declaration must reach the IIFE body: {s}"
        );
        assert!(
            s.contains("return (\nx\n)"),
            "expected `x` auto-returned: {s}"
        );
    }

    /// The regex fix end to end: every wrap path must now emit valid JS for
    /// `/a;b/.test("a;b")`. Firefox is the real judge (see
    /// `live_167_regex_literal_survives_every_wrap`); this pins the generated
    /// shape so a regression is caught without a browser.
    #[test]
    fn unit_167_regex_script_is_never_split() {
        let script = r#"/a;b/.test("a;b")"#;
        assert!(
            looks_like_single_expression(script),
            "a regex-literal call is one expression"
        );

        // --stringify: the script lands whole in the helper's argument slot,
        // with no IIFE split around the regex.
        let s = build_script(script, true, false);
        assert!(
            s.contains(&format!("}})({script});")),
            "a single expression must go straight into the argument slot: {s}"
        );
        assert!(!s.contains("/a;\n"), "the regex must not be split: {s}");

        // await: the wrap must be the single-expression `return (…)` form.
        let awaited = format!("await Promise.resolve({script})");
        let s = build_script(&awaited, false, true);
        assert_eq!(
            s,
            format!("(async function(){{return (\n{awaited}\n);}})()")
        );

        // plain: declaration-free, so still byte-for-byte passthrough.
        assert_eq!(build_script(script, false, true), script);
    }

    /// A block comment containing a line terminator is an ASI boundary
    /// candidate in the same way the newline it hides would have been —
    /// `1 /* x\ny */ 2` is two statements, not one. Skipping the comment
    /// wholesale without this check would have made the scanner *lose* a
    /// boundary it used to find.
    #[test]
    fn unit_167_multiline_block_comment_is_still_an_asi_boundary() {
        let script = "const x = 1 /* note\nmore */ x";
        let boundaries = top_level_statement_boundaries(script);
        assert_eq!(
            boundaries.len(),
            1,
            "expected exactly one boundary (no duplicates), got {boundaries:?}"
        );
        assert_eq!(
            &script[boundaries[0]..],
            "x",
            "the boundary must land on the statement after the comment"
        );
    }

    // ── iter-170: `${…}` interpolation and the `}` ambiguity ─────────────────

    /// AC: `unit_170_interpolation_is_scanned`.
    ///
    /// The headline defect. Measured against a live Firefox (2026-08-17):
    /// ``eval --stringify 'const s = `a${"`"}b`; s'`` returned
    /// `{"type":"undefined"}` instead of ``a`b``. The interpolated backtick
    /// closed the template, the `"` after it opened double-quote state, and
    /// the script's real top-level `;` was swallowed as string content — so no
    /// boundary was reported, the iter-165 wrap had nothing to auto-return,
    /// and the value was silently lost.
    #[test]
    fn unit_170_interpolation_is_scanned() {
        // A backtick, a quote and a `;` inside an interpolation are all
        // interior to the template literal: none of them ends it, and none is
        // a top-level boundary.
        for script in [
            r#"`a${"`"}b`"#,
            r"`a${'`'}b`",
            r#"`a${"x;y"}b`"#,
            r#"`a${ ";" }b`"#,
            "`a${ `n${1}m` }b`",
            r#"`v=${JSON.stringify({a:";"})}`"#,
            // A regex, a comment and a nested object literal inside the
            // interpolation are code, and are now scanned as code.
            r#"`m=${/a;b/.test("a;b")}`"#,
            "`m=${ 1 /* a; b */ + 2 }`",
            "`m=${ {a: 1}.a }`",
        ] {
            assert!(
                top_level_statement_boundaries(script).is_empty(),
                "{script:?} is one template-literal expression, got {:?}",
                top_level_statement_boundaries(script)
            );
        }

        // The whole point: the real top-level `;` after the template is still
        // found, so the wrap can auto-return the trailing expression.
        let script = r#"const s = `a${"`"}b`; s"#;
        let boundaries = top_level_statement_boundaries(script);
        assert_eq!(
            boundaries,
            vec![r#"const s = `a${"`"}b`;"#.len()],
            "the `;` after the template is a real boundary, got {boundaries:?}"
        );
        assert!(
            declares_at_top_level(script),
            "the `const` must still be seen"
        );
        assert!(
            build_script(script, true, true).contains("return (\ns\n)"),
            "the trailing `s` must be auto-returned, not silently dropped: {}",
            build_script(script, true, true)
        );

        // A `;` *inside* an interpolation is never a top-level boundary, even
        // though it now sits in a re-entered code context.
        assert!(
            top_level_statement_boundaries("`a${ (()=>{let q=1; return q})() }b`").is_empty(),
            "an interpolation's own `;` is nested, not top-level"
        );
    }

    /// iter-170 Theme C: a `/` after a *block*'s `}` opens a regex literal; a
    /// `/` after an *object literal*'s `}` is a division.
    ///
    /// Measured on `main` (2026-08-17):
    /// `eval --stringify 'const n = 1; if (n) {} /a;b/.test("a;b")'` failed
    /// with `unterminated regular expression literal`, because the `/` was
    /// scanned as a division and the `;` inside the regex was reported as a
    /// top-level boundary.
    #[test]
    fn unit_170_brace_kind_decides_regex_after_close() {
        // Block `}` then a regex: the `;` *inside* the regex must not be a
        // boundary, and the two real boundaries are the `;` after
        // `const n = 1` and the block-terminated start of the regex statement.
        for script in [
            r#"const n = 1; if (n) {} /a;b/.test("a;b")"#,
            "const n = 1; while (n) {} /a;b/.source",
            "const n = 1; try {} finally {} /a;b/.source",
            "const n = 1; { } /a;b/.source",
        ] {
            let boundaries = top_level_statement_boundaries(script);
            let regex_at = script.find("/a;b/").expect("the regex is in every case");
            assert_eq!(
                boundaries,
                vec!["const n = 1;".len(), regex_at],
                "{script:?}: expected the `;` boundary and the block-terminated \
                 regex statement, got {boundaries:?}"
            );
        }

        // Object-literal `}` then a division: unchanged from iter-167, and the
        // direction that must not regress — reading this `/` as a regex would
        // swallow the following `;`.
        let script = "const o = {v: 8}; o.v / 2; o.v / 4";
        let boundaries = top_level_statement_boundaries(script);
        assert_eq!(
            boundaries,
            vec![
                "const o = {v: 8};".len(),
                "const o = {v: 8}; o.v / 2;".len()
            ],
            "division after an object literal must keep both boundaries, got {boundaries:?}"
        );
        for script in ["({a: 1}) / 2", "x = {} / 2", "f({}) / 2", "(() => {}) / 2"] {
            assert!(
                top_level_statement_boundaries(script).is_empty(),
                "{script:?} is a single division expression, got {:?}",
                top_level_statement_boundaries(script)
            );
        }

        // An unbalanced `}` leaves the brace kind unknown, which keeps
        // iter-167's answer (division) rather than guessing.
        assert!(
            !slash_starts_regex(
                &"} / 2".char_indices().collect::<Vec<_>>(),
                Some('}'),
                Some(0),
                BraceKind::Unknown
            ),
            "an unknown brace kind must fall back to division"
        );
    }

    /// iter-170 Theme C, second half: a block statement terminates itself, so
    /// the token after its `}` starts a new top-level statement even with no
    /// `;` and no newline. Only decidable once [`brace_opens_block`] exists —
    /// before iter-170 the scanner could not tell `if (x) {} y` from
    /// `({a:1}) / 2`.
    #[test]
    fn unit_170_block_close_ends_a_statement() {
        for (script, expected_tail) in [
            ("if (n) {} foo()", "foo()"),
            ("function f() {} f()", "f()"),
            ("for (const a of xs) {} done()", "done()"),
            ("if (n) { const y = 1 } y2()", "y2()"),
            ("{ } bare", "bare"),
        ] {
            let boundaries = top_level_statement_boundaries(script);
            assert_eq!(
                boundaries.len(),
                1,
                "{script:?}: expected exactly one block boundary, got {boundaries:?}"
            );
            assert_eq!(
                &script[boundaries[0]..],
                expected_tail,
                "{script:?}: the boundary must land on the statement after the block"
            );
        }

        // Suppressed: nothing after the block can start a statement, so the
        // scanner keeps its pre-170 answer rather than inventing a split.
        for script in [
            "if (n) {} else {}",
            "try {} catch (e) {}",
            "try {} finally {}",
            "do {} while (n)",
            // The IIFE forms: `(`/`[`/backtick continue the preceding value.
            "!function () {}()",
            "const f = function () {}",
            "const f = function () {}, g = 2",
            "if (n) {} // trailing note",
            "if (n) {} /* trailing note */",
            // Object literals are unaffected — their `}` ends an expression.
            "const o = {v: 8}",
            "x = {a: 1}",
        ] {
            assert!(
                top_level_statement_boundaries(script).is_empty(),
                "{script:?} must not gain a boundary, got {:?}",
                top_level_statement_boundaries(script)
            );
        }

        // The rule must not turn a block-scoped declaration into a top-level
        // one — `eval --help` promises `if (1) { const z = 2 }` skips the wrap.
        assert!(!declares_at_top_level("if (1) { const z = 2 }"));
        assert!(!declares_at_top_level("for (const a of xs) {}"));
        assert!(!declares_at_top_level("if (1) { const z = 2 } foo()"));
    }

    /// iter-170: the whole reason gap 1 was worth fixing — the emitted script
    /// stops losing the value, and gap 2's emitted script stops being invalid
    /// JavaScript. Both are checked on `build_script` output rather than on
    /// boundaries alone, because the boundary list is only the input to the
    /// wrap that the user actually sees.
    #[test]
    fn unit_170_wrapped_scripts_are_valid_and_return_a_value() {
        // Gap 2: the split used to fall inside the regex literal, emitting
        // `… {} /a;` + `return (b/.test("a;b"))`.
        let gap2 = r#"const n = 1; if (n) {} /a;b/.test("a;b")"#;
        let built = build_script(gap2, true, true);
        assert!(
            !built.contains("/a;\n"),
            "the regex literal must not be split across the wrap: {built}"
        );
        assert!(
            built.contains(r#"/a;b/.test("a;b")"#),
            "the regex must survive the wrap intact: {built}"
        );

        // Gap 1: the template used to end at the interpolated backtick, so the
        // trailing `s` was never recognized as the last statement.
        let gap1 = r#"const s = `a${"`"}b`; s"#;
        let built = build_script(gap1, false, true);
        assert!(
            built.contains("return (\ns\n)"),
            "the trailing expression must be auto-returned: {built}"
        );
    }

    /// Review fix (post-iter-170): a `function` *expression*'s `}` — reached
    /// at the same depth its `function` keyword was seen at — must not be
    /// classified the way a declaration's is. Before this fix,
    /// `top_level_statement_boundaries` classified `)`-preceded `{` as
    /// `Block` regardless of whether the enclosing `function` was a
    /// declaration or an expression, so `const f = function(){} / 2` (one
    /// division statement) gained a spurious boundary right at the `/` and
    /// `eval --stringify 'const f = function(){} / 2'` threw
    /// `unterminated regular expression literal` on a script that evaluates
    /// fine unwrapped. Measured live against both `main` (pre-iter-170,
    /// works) and this branch pre-fix (throws); this fix restores parity.
    #[test]
    fn unit_170_function_expression_body_is_not_a_statement_block() {
        // No boundary at all: each is one statement (a division, or a
        // division whose value is discarded as a declarator initializer).
        for script in [
            "const f = function(){} / 2",
            "const f = function(){} / a/b",
            "let f = function(){} / 2",
            "const f = function named(){} / 2",
            "const f = async function(){} / 2",
            "const f = function* (){} / 2",
        ] {
            assert!(
                top_level_statement_boundaries(script).is_empty(),
                "{script:?} is one statement (division), got {:?}",
                top_level_statement_boundaries(script)
            );
        }

        // A callback argument's function-expression body must not gain a
        // boundary from what follows the call either.
        assert!(
            top_level_statement_boundaries("arr.map(function(x) { return x; }) / 2").is_empty(),
            "a callback's body must not be mistaken for a statement block"
        );

        // Declarations are unaffected — `function`/`async function` at true
        // statement position still get the iter-170 self-terminating
        // boundary and regex-permitting `/`.
        for (script, expected_tail) in [
            ("function f(){ return 5 } f()", "f()"),
            ("async function f(){ return 5 } f()", "f()"),
        ] {
            let boundaries = top_level_statement_boundaries(script);
            assert_eq!(
                boundaries.len(),
                1,
                "{script:?}: a declaration's `}}` must still end its statement, got {boundaries:?}"
            );
            assert_eq!(&script[boundaries[0]..], expected_tail);
        }

        // The wrapped script must not become invalid JavaScript — this is
        // the observable the live test checks against a real Firefox.
        let built = build_script("const f = function(){} / 2", false, true);
        assert!(
            !built.contains("return (\n/"),
            "a division must not be split into an unterminated regex: {built}"
        );
    }
}
