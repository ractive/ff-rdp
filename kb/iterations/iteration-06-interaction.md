---
title: "Iteration 6: Interaction Commands"
type: iteration
date: 2026-04-06
tags:
  - iteration
  - click
  - type
  - wait
  - interaction
status: completed
branch: iter-6/interaction
---

# Iteration 6: Interaction Commands

Click elements, type into inputs, and wait for conditions — enabling automated form filling and UI testing.

## Tasks

- [x] Implement `ff-rdp-cli/src/commands/click.rs` — `ff-rdp click <selector> [--tab ...]`
- [x] Implement `ff-rdp-cli/src/commands/type_text.rs` — `ff-rdp type <selector> <text> [--tab ...] [--clear]`
- [x] Implement `ff-rdp-cli/src/commands/wait.rs` — `ff-rdp wait [--selector <css>] [--text <text>] [--eval <js>] [--timeout <ms>] [--tab ...]`
- [x] Click implementation via eval: `document.querySelector(sel).click()`
- [x] Type implementation via eval: set `.value`, dispatch `input` and `change` events
- [x] Wait implementation: poll with eval in a loop with configurable interval and timeout
- [x] Handle element-not-found errors gracefully (exit 1 with clear message)

## Implementation Notes

All interaction commands use `eval` internally. This is simpler and more reliable than dispatching synthetic DOM events through the protocol.

```javascript
// click
(() => {
  const el = document.querySelector(selector);
  if (!el) throw new Error('Element not found: ' + selector);
  el.click();
  return {clicked: true, tag: el.tagName, text: el.textContent.slice(0, 100)};
})()

// type (with --clear)
(() => {
  const el = document.querySelector(selector);
  if (!el) throw new Error('Element not found: ' + selector);
  el.value = text;
  el.dispatchEvent(new Event('input', {bubbles: true}));
  el.dispatchEvent(new Event('change', {bubbles: true}));
  return {typed: true, value: el.value};
})()

// wait --selector ".loaded" --timeout 5000
// Polls every 100ms until selector matches or timeout
```

## Acceptance Criteria

- `ff-rdp click "button.submit"` clicks the button, returns confirmation
- `ff-rdp type "input[name=email]" "test@example.com"` fills the input
- `ff-rdp type "input[name=email]" "new@example.com" --clear` clears then fills
- `ff-rdp wait --selector ".results"` blocks until element appears
- `ff-rdp wait --eval "document.readyState === 'complete'"` waits for page load
- `ff-rdp wait --text "Success" --timeout 10000` waits for text to appear on page
- All commands return structured JSON confirming the action
- Element-not-found produces clear error with the selector in the message
