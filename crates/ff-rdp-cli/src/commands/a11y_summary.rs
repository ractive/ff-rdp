use serde_json::{Value, json};

use crate::cli::args::Cli;
use crate::error::AppError;
use crate::hints::{HintContext, HintSource};
use crate::output;
use crate::output_controls::{OutputControls, SortDir};
use crate::output_pipeline::OutputPipeline;

use super::connect_tab::connect_direct;
use super::js_helpers::{eval_or_bail, resolve_result};

const A11Y_SUMMARY_JS: &str = r#"(function() {
  var result = {landmarks: [], headings: [], interactive: []};

  // Landmarks
  var landmarkRoles = ['banner','navigation','main','contentinfo','complementary','search','form'];
  var landmarkTags = {HEADER:'banner',NAV:'navigation',MAIN:'main',FOOTER:'contentinfo',ASIDE:'complementary'};

  // Check role attributes
  landmarkRoles.forEach(function(role) {
    var els = document.querySelectorAll('[role="' + role + '"]');
    for (var i = 0; i < els.length; i++) {
      var label = els[i].getAttribute('aria-label') || '';
      result.landmarks.push({role: role, label: label, tag: els[i].tagName.toLowerCase()});
    }
  });
  // Check semantic HTML (only if no explicit role already captured)
  Object.keys(landmarkTags).forEach(function(tag) {
    var els = document.getElementsByTagName(tag);
    for (var i = 0; i < els.length; i++) {
      if (!els[i].getAttribute('role')) {
        var label = els[i].getAttribute('aria-label') || '';
        result.landmarks.push({role: landmarkTags[tag], label: label, tag: tag.toLowerCase()});
      }
    }
  });

  // Headings
  for (var level = 1; level <= 6; level++) {
    var headings = document.querySelectorAll('h' + level);
    for (var j = 0; j < headings.length; j++) {
      var text = headings[j].textContent.trim();
      if (text.length > 100) text = text.slice(0, 100) + '...';
      result.headings.push({level: level, text: text});
    }
  }

  // Interactive: links
  var links = document.querySelectorAll('a[href]');
  for (var k = 0; k < links.length; k++) {
    var linkText = links[k].textContent.trim();
    if (linkText.length > 100) linkText = linkText.slice(0, 100) + '...';
    result.interactive.push({role: 'link', name: linkText, href: links[k].getAttribute('href')});
  }

  // Interactive: buttons
  var buttons = document.querySelectorAll('button, [role="button"], input[type="button"], input[type="submit"]');
  for (var m = 0; m < buttons.length; m++) {
    var btnText = buttons[m].textContent.trim() || buttons[m].getAttribute('aria-label') || buttons[m].value || '';
    if (btnText.length > 100) btnText = btnText.slice(0, 100) + '...';
    result.interactive.push({role: 'button', name: btnText});
  }

  // Interactive: inputs (text, email, password, etc.)
  var inputs = document.querySelectorAll('input:not([type="button"]):not([type="submit"]):not([type="hidden"]), textarea, select');
  for (var n = 0; n < inputs.length; n++) {
    var inp = inputs[n];
    var inputName = '';
    if (inp.labels && inp.labels.length) inputName = inp.labels[0].textContent.trim();
    if (!inputName) inputName = inp.getAttribute('aria-label') || inp.getAttribute('placeholder') || inp.getAttribute('name') || '';
    if (inputName.length > 100) inputName = inputName.slice(0, 100) + '...';
    var inputType = inp.getAttribute('type') || inp.tagName.toLowerCase();
    result.interactive.push({role: 'input', name: inputName, type: inputType});
  }

  return '__FF_RDP_JSON__' + JSON.stringify(result);
})()"#;

pub fn run(cli: &Cli) -> Result<(), AppError> {
    let mut ctx = connect_direct(cli)?;
    let console_actor = ctx.target.console_actor.clone();

    let eval_result = eval_or_bail(
        &mut ctx,
        &console_actor,
        A11Y_SUMMARY_JS,
        "a11y summary failed",
    )?;
    let results = resolve_result(&mut ctx, &eval_result.result)?;

    // Apply --limit to interactive elements if set.
    let controls = OutputControls::from_cli(cli, SortDir::Asc);
    let mut output_results = results;

    if let Some(Value::Array(arr)) = output_results.get_mut("interactive") {
        let orig_len = arr.len();
        let (limited, _, truncated) = controls.apply_limit(std::mem::take(arr), Some(50));
        *arr = limited;
        if truncated && let Some(obj) = output_results.as_object_mut() {
            obj.insert("interactive_total".to_string(), json!(orig_len));
            obj.insert("interactive_truncated".to_string(), json!(true));
        }
    }

    let meta = json!({"host": cli.host, "port": cli.port});
    let envelope = output::envelope(&output_results, 1, &meta);

    // Custom text rendering for a11y summary.
    if cli.format == "text" && cli.jq.is_none() {
        render_summary_text(&output_results);
        return Ok(());
    }

    let hint_ctx = HintContext::new(HintSource::A11ySummary);
    OutputPipeline::from_cli(cli)?
        .finalize_with_hints(&envelope, Some(&hint_ctx))
        .map_err(AppError::from)
}

fn render_summary_text(results: &Value) {
    // Landmarks
    if let Some(landmarks) = results.get("landmarks").and_then(Value::as_array)
        && !landmarks.is_empty()
    {
        println!("LANDMARKS");
        for lm in landmarks {
            let role = lm.get("role").and_then(Value::as_str).unwrap_or("?");
            let tag = lm.get("tag").and_then(Value::as_str).unwrap_or("");
            let label = lm.get("label").and_then(Value::as_str).unwrap_or("");
            if label.is_empty() {
                println!("  {role} <{tag}>");
            } else {
                println!("  {role} <{tag}> \"{label}\"");
            }
        }
        println!();
    }

    // Headings
    if let Some(headings) = results.get("headings").and_then(Value::as_array)
        && !headings.is_empty()
    {
        println!("HEADINGS");
        for h in headings {
            let level = h.get("level").and_then(Value::as_u64).unwrap_or(0);
            let text = h.get("text").and_then(Value::as_str).unwrap_or("");
            let indent = "  ".repeat(usize::try_from(level).unwrap_or(0));
            println!("{indent}h{level} {text}");
        }
        println!();
    }

    // Interactive
    if let Some(interactive) = results.get("interactive").and_then(Value::as_array)
        && !interactive.is_empty()
    {
        println!("INTERACTIVE ({} elements)", interactive.len());
        for el in interactive {
            let role = el.get("role").and_then(Value::as_str).unwrap_or("?");
            let name = el.get("name").and_then(Value::as_str).unwrap_or("");
            match role {
                "link" => {
                    let href = el.get("href").and_then(Value::as_str).unwrap_or("");
                    println!("  link \"{name}\" -> {href}");
                }
                "button" => {
                    println!("  button \"{name}\"");
                }
                "input" => {
                    let itype = el.get("type").and_then(Value::as_str).unwrap_or("text");
                    println!("  input[{itype}] \"{name}\"");
                }
                _ => {
                    println!("  {role} \"{name}\"");
                }
            }
        }
        if let Some(true) = results
            .get("interactive_truncated")
            .and_then(Value::as_bool)
        {
            let total = usize::try_from(
                results
                    .get("interactive_total")
                    .and_then(Value::as_u64)
                    .unwrap_or(0),
            )
            .unwrap_or(0);
            println!(
                "  ... and {} more (use --all for complete list)",
                total.saturating_sub(interactive.len())
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a11y_summary_js_has_sentinel() {
        assert!(
            A11Y_SUMMARY_JS.contains("__FF_RDP_JSON__"),
            "JS template must use sentinel prefix"
        );
    }

    #[test]
    fn a11y_summary_js_uses_json_stringify() {
        assert!(
            A11Y_SUMMARY_JS.contains("JSON.stringify"),
            "JS template must use JSON.stringify"
        );
    }

    #[test]
    fn a11y_summary_js_collects_all_sections() {
        assert!(
            A11Y_SUMMARY_JS.contains("landmarks"),
            "JS must collect landmarks"
        );
        assert!(
            A11Y_SUMMARY_JS.contains("headings"),
            "JS must collect headings"
        );
        assert!(
            A11Y_SUMMARY_JS.contains("interactive"),
            "JS must collect interactive"
        );
    }

    #[test]
    fn render_summary_text_landmarks() {
        let results = json!({
            "landmarks": [
                {"role": "banner", "tag": "header", "label": ""},
                {"role": "main", "tag": "main", "label": "Content"}
            ],
            "headings": [],
            "interactive": []
        });
        // Should not panic; call to exercise code paths.
        render_summary_text(&results);
    }

    #[test]
    fn render_summary_text_headings() {
        let results = json!({
            "landmarks": [],
            "headings": [
                {"level": 1, "text": "Page Title"},
                {"level": 2, "text": "Section"},
                {"level": 3, "text": "Subsection"}
            ],
            "interactive": []
        });
        render_summary_text(&results);
    }

    #[test]
    fn render_summary_text_interactive_all_roles() {
        let results = json!({
            "landmarks": [],
            "headings": [],
            "interactive": [
                {"role": "link", "name": "Home", "href": "/"},
                {"role": "button", "name": "Submit"},
                {"role": "input", "name": "Email", "type": "email"},
                {"role": "unknown", "name": "Widget"}
            ]
        });
        render_summary_text(&results);
    }

    #[test]
    fn render_summary_text_truncation_note() {
        let results = json!({
            "landmarks": [],
            "headings": [],
            "interactive": [
                {"role": "link", "name": "Link 1", "href": "/1"},
                {"role": "link", "name": "Link 2", "href": "/2"}
            ],
            "interactive_total": 10,
            "interactive_truncated": true
        });
        render_summary_text(&results);
    }

    #[test]
    fn render_summary_text_empty_sections_no_output() {
        // When all sections are empty the function should run without panic.
        let results = json!({
            "landmarks": [],
            "headings": [],
            "interactive": []
        });
        render_summary_text(&results);
    }
}
