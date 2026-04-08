use ff_rdp_core::{ActorId, EvalResult, RdpTransport, WebConsoleActor};

use crate::error::AppError;

/// Evaluate `js` via the WebConsole actor and return the [`EvalResult`] on
/// success.  If the evaluation threw a JS exception, print `error: <msg>` to
/// stderr and return [`AppError::Exit(1)`].
///
/// Use this for commands that treat a JS exception as a fatal error that
/// should cause a non-zero process exit (e.g. `eval`, `click`, `snapshot`).
pub(crate) fn eval_or_bail(
    transport: &mut RdpTransport,
    console_actor: &ActorId,
    js: &str,
) -> Result<EvalResult, AppError> {
    let eval_result =
        WebConsoleActor::evaluate_js_async(transport, console_actor, js).map_err(AppError::from)?;

    if let Some(ref exc) = eval_result.exception {
        let msg = exc
            .message
            .as_deref()
            .unwrap_or("evaluation threw an exception");
        eprintln!("error: {msg}");
        return Err(AppError::Exit(1));
    }

    Ok(eval_result)
}

/// Evaluate `js` via the WebConsole actor and return the [`EvalResult`] on
/// success.  If the evaluation threw a JS exception, return
/// [`AppError::User`] with the message formatted as `"{context}: {msg}"`.
///
/// Use this for commands that treat a JS exception as a user-visible error
/// (e.g. `perf`, `responsive`, `a11y`) where a structured error message is
/// more helpful than a raw exit code.
pub(crate) fn eval_or_user_error(
    transport: &mut RdpTransport,
    console_actor: &ActorId,
    js: &str,
    context: &str,
) -> Result<EvalResult, AppError> {
    let eval_result =
        WebConsoleActor::evaluate_js_async(transport, console_actor, js).map_err(AppError::from)?;

    if let Some(ref exc) = eval_result.exception {
        let msg = exc
            .message
            .as_deref()
            .unwrap_or("evaluation threw an exception");
        return Err(AppError::User(format!("{context}: {msg}")));
    }

    Ok(eval_result)
}
