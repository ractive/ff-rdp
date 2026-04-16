use serde_json::json;

use crate::cli::args::Cli;
use crate::error::AppError;
use crate::output;
use crate::output_pipeline::OutputPipeline;

const LLM_REFERENCE: &str = r##"# ff-rdp — Firefox Remote Debugging Protocol CLI

## Global flags
  --host <HOST>          Firefox debug server host [default: localhost]
  --port <PORT>          Firefox debug server port [default: 6000]
  --tab <TAB>            Target tab by index (1-based) or URL substring
  --tab-id <TAB_ID>     Target tab by exact actor ID
  --jq <FILTER>          jq filter expression applied to JSON output
  --timeout <MS>         Operation timeout in milliseconds [default: 5000]
  --no-daemon            Connect directly to Firefox (no daemon)
  --daemon-timeout <S>   Daemon idle timeout in seconds [default: 300]
  --allow-unsafe-urls    Allow javascript: and data: URL schemes
  --format <FORMAT>      Output format: "json" (default) or "text" for human-readable tables

## Output control flags (global, apply to all list-returning commands)
  --limit <N>            Limit number of results (per-command defaults apply)
  --all                  Return all results, overriding default limit
  --sort <FIELD>         Sort results by field name
  --asc                  Sort ascending
  --desc                 Sort descending
  --fields <F1,F2,...>   Select which fields to include in each entry
  --detail               Show individual entries instead of summary mode
  --format text          Human-readable table output (mutually exclusive with --jq)

Commands default to a summary view. Use --detail for per-entry output.
Defaults: network (limit 20, sort duration_ms desc), console (limit 50,
sort timestamp desc), dom (limit 20, document order), perf resource
(limit 20, sort duration_ms desc). Use --all to get everything.

## Commands

### tabs
List open browser tabs.
```
ff-rdp tabs
ff-rdp tabs --jq '.[].url'
```

### navigate <URL>
Navigate to a URL.
  --with-network         Also capture network requests during navigation
  --wait-text <TEXT>     Wait for text to appear after navigation
  --wait-selector <SEL>  Wait for CSS selector to match after navigation
  --wait-timeout <MS>    Timeout for wait condition [default: 5000]
```
ff-rdp navigate https://example.com
ff-rdp navigate https://example.com --with-network
ff-rdp navigate https://example.com --wait-text "Welcome"
```

### eval <SCRIPT>
Evaluate JavaScript in the target tab.
Three input modes (exactly one required):
  Positional:  ff-rdp eval 'document.title'
  From file:   ff-rdp eval --file script.js
  From stdin:  echo 'document.title' | ff-rdp eval --stdin
Use --file or --stdin to avoid shell quoting issues with ?., template literals, or multi-line scripts.
  --file <PATH>          Read JavaScript from a file
  --stdin                Read JavaScript from stdin until EOF
```
ff-rdp eval "document.title"
ff-rdp eval "document.querySelectorAll('a').length"
ff-rdp eval --file script.js
echo 'document.body?.dataset.theme' | ff-rdp eval --stdin
```

### page-text
Extract visible page text (document.body.innerText).
```
ff-rdp page-text
ff-rdp page-text --jq 'split("\n") | map(select(length > 0))'
```

### dom <SELECTOR>
Query DOM elements by CSS selector.
  --outer-html           Output outer HTML (default)
  --inner-html           Output inner HTML
  --text                 Output text content only
  --attrs                Output element attributes as JSON objects
  --count                Return only the number of matching elements
```
ff-rdp dom "h1" --text
ff-rdp dom "img" --attrs --jq '.[].src'
ff-rdp dom "script" --count
```

### dom stats
DOM statistics: node count, document size, inline scripts, render-blocking resources.
```
ff-rdp dom stats
ff-rdp dom stats --jq '.results.node_count'
```

### dom tree [SELECTOR]
Dump structured DOM subtree via native WalkerActor (not JS eval).
  --depth <N>            Maximum tree depth [default: 6]
  --max-chars <N>        Maximum characters of text content [default: 50000]
```
ff-rdp dom tree
ff-rdp dom tree "main" --depth 3
ff-rdp dom tree ".content" --max-chars 10000
ff-rdp dom tree --jq '.results.children | map(.nodeName)'
```

### computed <SELECTOR>
Quick wrapper around getComputedStyle() for CSS debugging.
Returns non-default computed style properties for every element matching the selector.
  --prop <NAME>          Return a single property value (e.g. "color", "display")
  --all                  Include every resolved property, not just non-default values
```
ff-rdp computed h1
ff-rdp computed h1 --prop color
ff-rdp computed .card --all
ff-rdp computed .card --jq '.results[].computed.display'
```

### styles <SELECTOR>
Inspect CSS styles for an element matching a CSS selector.
Default: computed styles as JSON array of {name, value, priority}.
  --applied              Show applied CSS rules with source locations
  --layout               Show box model (margin/border/padding/content per side)
```
ff-rdp styles "h1"
ff-rdp styles "h1" --jq '.results[] | select(.name == "color")'
ff-rdp styles "h1" --applied
ff-rdp styles "h1" --applied --jq '.results[].selector'
ff-rdp styles "h1" --layout
ff-rdp styles "h1" --layout --jq '.results.margin'
```

### console
Read console messages.
Output always includes a "summary" field: {total, matched, shown, by_level}.
"total" is the raw message count before any filter; "matched" is after filter but before --limit;
"shown" is actual results length. Use this to distinguish "no errors" from "filter was too narrow".
  --level <LEVEL>        Filter by log level (error, warn, info, log, debug)
  --pattern <REGEX>      Filter by message content (regex)
  --follow               Stream console messages in real time (NDJSON, Ctrl-C to stop)
```
ff-rdp console --level error
ff-rdp console --pattern "API"
ff-rdp console --limit 10 --jq '.summary'
ff-rdp console --follow
ff-rdp console --follow --level warn
```

### network
Show network requests captured by WatcherActor.
Default: summary view (counts, totals, top 20 slowest). Use --detail for per-request entries.
  --filter <URL>         Filter by URL pattern (substring)
  --method <METHOD>      Filter by HTTP method
  --follow               Stream network events in real time (NDJSON, Ctrl-C to stop)
```
ff-rdp network
ff-rdp network --detail
ff-rdp network --detail --limit 10
ff-rdp network --filter ".js" --method GET --detail
ff-rdp network --follow
ff-rdp network --follow --filter ".js"
```

### perf [--type <TYPE>] [--filter <URL>]
Query Performance API entries.
Types: resource, navigation, paint, lcp, cls, longtask
  --filter <URL>         Filter by URL substring (resource/navigation)

Subcommands:
  perf vitals            Core Web Vitals summary (LCP, CLS, TBT, FCP, TTFB)
  perf summary           Aggregated resource summary (sizes, counts, domains)
  perf audit             Audit performance with actionable recommendations
  perf compare <URL>...  Compare performance across multiple URLs
    --label <L1,L2,...>  Labels for each URL in output
```
ff-rdp perf --type resource
ff-rdp perf --type resource --filter ".js"
ff-rdp perf --type resource --group-by domain
ff-rdp perf vitals
ff-rdp perf summary
ff-rdp perf audit
ff-rdp perf compare https://a.example https://b.example
ff-rdp perf compare https://a.example https://b.example --label "Site A,Site B"
```

### screenshot
Capture a screenshot.
  -o, --output <PATH>    Output file path
  --base64               Return screenshot as base64 PNG in JSON output (no file saved)
  --full-page            Capture entire scrollable page (document.scrollingElement.scrollHeight)
  --viewport-height <PX> Capture at explicit pixel height (alternative to --full-page)
```
ff-rdp screenshot -o page.png
ff-rdp screenshot --base64
ff-rdp screenshot --full-page -o full.png
ff-rdp screenshot --viewport-height 2000 -o tall.png
```

### snapshot
Dump structured page snapshot for LLM consumption: DOM tree with semantic roles,
key attributes, interactive elements, and text content.
  --depth <N>            Maximum tree depth to traverse [default: 6]
  --max-chars <N>        Maximum total characters of text content [default: 50000]
```
ff-rdp snapshot
ff-rdp snapshot --depth 3
ff-rdp snapshot --jq '.results.children[0]'
```

### a11y
Inspect accessibility tree via Firefox's AccessibilityActor.
  --depth <N>            Maximum tree depth [default: 6]
  --max-chars <N>        Maximum characters of text content [default: 50000]
  --selector <SEL>       Root tree at a specific CSS selector
  --interactive          Only show interactive elements (buttons, links, inputs)
```
ff-rdp a11y
ff-rdp a11y --depth 3
ff-rdp a11y --selector ".main-content"
ff-rdp a11y --interactive
ff-rdp a11y --jq '.results.children[] | select(.role == "link")'
```

### a11y contrast
Check WCAG color contrast ratios for text elements.
  --selector <SEL>       CSS selector to limit checking [default: all]
  --fail-only            Only show elements that fail AA contrast
```
ff-rdp a11y contrast
ff-rdp a11y contrast --selector "h1,p,a"
ff-rdp a11y contrast --fail-only
ff-rdp a11y contrast --jq '.meta.summary'
```

### geometry <SELECTOR>...
Get element geometry: bounding rects, position, z-index, visibility, overflow,
with automatic overlap detection between elements.
```
ff-rdp geometry "h1" "p"
ff-rdp geometry ".modal" ".overlay" --jq '.results.overlaps'
```

### responsive <SELECTOR>...
Test responsive layout across viewport widths: resize to each width, collect geometry
+ key computed styles (flex, grid, font-size), then restore original viewport.
  --widths <W1,W2,...>   Viewport widths to test [default: 320,768,1024,1440]
```
ff-rdp responsive "h1" "nav" ".sidebar"
ff-rdp responsive "h1" --widths 320,768,1440
ff-rdp responsive ".card" --jq '.results.breakpoints[] | {width, elements: [.elements[] | {selector, rect}]}'
```

### click <SELECTOR>
Click an element matching a CSS selector.
```
ff-rdp click "button.submit"
```

### type <SELECTOR> <TEXT>
Type text into an input element.
  --clear                Clear current value before typing
```
ff-rdp type "input[name=search]" "hello" --clear
```

### wait
Wait for a condition (polls every 100ms). Exactly one condition required.
  --selector <SEL>       Wait for CSS selector to exist
  --text <TEXT>           Wait for text on page
  --eval <JS>            Wait for JS expression to be truthy
  --wait-timeout <MS>    Timeout [default: 5000]
```
ff-rdp wait --selector ".loaded"
ff-rdp wait --text "Success" --wait-timeout 10000
```

### cookies
List cookies via StorageActor.
  --name <NAME>          Filter by cookie name (exact match)
```
ff-rdp cookies
ff-rdp cookies --name "session_id"
```

### storage <TYPE>
Read web storage (local or session).
  --key <KEY>            Get a specific key only
```
ff-rdp storage local
ff-rdp storage session --key "token"
```

### reload
Reload the current page.
  --wait-idle            Block until network is idle after reload (replaces sleep)
  --idle-ms <MS>         Milliseconds of inactivity that counts as idle [default: 500]
  --reload-timeout <MS>  Maximum total wait time [default: 10000]
Output with --wait-idle: {"results": {"reloaded": true, "idle_at_ms": N, "requests_observed": M}, ...}
```
ff-rdp reload
ff-rdp reload --wait-idle
ff-rdp reload --wait-idle --idle-ms 1000 --reload-timeout 30000
```

### back
Go back in browser history.
```
ff-rdp back
```

### forward
Go forward in browser history.
```
ff-rdp forward
```

### inspect <ACTOR_ID>
Inspect a remote JavaScript object by grip actor ID.
  --depth <N>            Recursion depth [default: 1]
```
ff-rdp inspect "conn0/obj123" --depth 2
```

### sources
List JavaScript/WASM sources loaded on the page.
  --filter <URL>         Filter by URL substring
  --pattern <REGEX>      Filter by URL regex
```
ff-rdp sources
ff-rdp sources --filter "main.js"
```

### scroll
Scroll the page or a specific element.

#### scroll to <SELECTOR>
Scroll an element into the viewport using scrollIntoView.
  --block <ALIGN>        Alignment: top, center, bottom, nearest [default: top]
  --smooth               Use smooth scrolling
```
ff-rdp scroll to ".listing:nth-child(5)"
ff-rdp scroll to ".listing:nth-child(5)" --block center
ff-rdp scroll to "footer" --smooth
```

#### scroll by
Scroll the viewport by pixels or by a page.
  --dx <PX>              Horizontal delta in pixels [default: 0]
  --dy <PX>              Vertical delta in pixels (mutually exclusive with --page-down/--page-up)
  --page-down            Scroll down by 85% of viewport height
  --page-up              Scroll up by 85% of viewport height
  --smooth               Use smooth scrolling
```
ff-rdp scroll by --page-down
ff-rdp scroll by --page-up
ff-rdp scroll by --dy 600 --smooth
ff-rdp scroll by --dx 200
```

#### scroll container <SELECTOR>
Scroll an overflow container element directly (scrollTop/scrollLeft).
  --dx <PX>              Horizontal delta [default: 0]
  --dy <PX>              Vertical delta [default: 0]
  --to-end               Scroll to bottom/right of container
  --to-start             Scroll to top/left of container
```
ff-rdp scroll container ".sidebar" --dy 300
ff-rdp scroll container ".feed" --to-end
ff-rdp scroll container ".panel" --to-start
```

#### scroll until <SELECTOR>
Scroll until an element is visible in the viewport (polls every 200ms).
  --direction <DIR>      up or down [default: down]
  --timeout <MS>         Timeout in milliseconds [default: 10000]
```
ff-rdp scroll until "#load-more-sentinel"
ff-rdp scroll until ".item:nth-child(50)" --timeout 15000
ff-rdp scroll until ".header" --direction up
```

#### scroll text <TEXT>
Find text on the page (case-sensitive) and scroll its container into view.
```
ff-rdp scroll text "Contact Us"
ff-rdp scroll text "Privacy Policy"
```

### launch
Launch Firefox with remote debugging enabled.
  --headless             Run in headless mode
  --profile <PATH>       Use specific profile directory
  --temp-profile         Create a temporary profile
  --debug-port <PORT>    Override debug port
  --auto-consent         Install Consent-O-Matic extension to auto-dismiss cookie consent banners (requires --profile or --temp-profile)
```
ff-rdp launch --headless --temp-profile
ff-rdp launch --headless --temp-profile --auto-consent
```

## Output format
All commands return JSON by default with envelope: `{"results": ..., "total": N, "meta": {...}}`
When results are truncated: `{"results": ..., "total": N, "truncated": true, "hint": "showing 20 of 84, use --all for complete list", "meta": {...}}`
Use `--jq` to filter: operates on the full envelope `{results, total, meta}` (use `.results`, `.total`, `.meta` to access fields; also implies --detail mode for list commands).
Use `--format text` for human-readable table output (mutually exclusive with --jq).

## Output examples

### tabs output
{"results": [{"url": "https://...", "title": "Page Title", "actor": "server1.conn0.tab1", "selected": true}], "total": 1, "meta": {"host": "localhost", "port": 6000}}

### network output (summary mode, default)
{"results": {"total_requests": 42, "total_transfer_bytes": 512000, "by_cause_type": {"script": 10, "img": 5}, "slowest": [...], "timeout_reached": false}, "total": 42, "meta": {...}}

### network output (detail mode: --detail, --jq, etc.)
{"results": [{"url": "...", "method": "GET", "status": 200, "duration_ms": 150}], "total": 42, "meta": {...}}

### console output
{"results": [{"level": "error", "message": "...", "source": "...", "line": 42, "timestamp": 1700000000}], "total": 5, "summary": {"total": 5, "matched": 5, "shown": 5, "by_level": {"error": 2, "warn": 3}}, "meta": {...}}

### dom output
{"results": ["<h1>Hello</h1>"], "total": 1, "meta": {...}}

### cookies output
{"results": [{"name": "session", "value": "abc", "domain": ".example.com", "path": "/", "secure": true, "httpOnly": true}], "total": 1, "meta": {...}}

## Troubleshooting

### Zero results
- `network` returns 0: page may have loaded before connection. Use `navigate --with-network <url>` or `network --follow`.
- `console` returns 0: no messages yet. Use `--follow` to stream, or generate with `eval 'console.log("test")'`.
- `cookies` returns 0: page may not set cookies, or a consent banner is blocking them. Check `meta.note` in the output.
- `dom` returns 0: selector may not match. Use `dom stats` to verify the page has content, then refine the selector.

### Timeout errors
- `wait`/`navigate --wait-*` timeout: the condition was not met. Increase `--wait-timeout` or verify the condition is correct.
- Connection timeout: Firefox may not be running or the port is wrong. Use `ff-rdp launch --headless --temp-profile` to start it.

### Tab not found
- Use `ff-rdp tabs` to list all available tabs and their indices/URLs.
- Tab index is 1-based (first tab is `--tab 1`).
- URL matching is case-insensitive substring: `--tab github` matches `https://github.com/...`.

## Workflow patterns

### Navigate and inspect
ff-rdp navigate https://example.com --wait-text "Welcome"
ff-rdp snapshot --depth 3
ff-rdp a11y --interactive

### Full page audit
ff-rdp navigate https://example.com --with-network
ff-rdp perf audit
ff-rdp a11y contrast --fail-only
ff-rdp screenshot -o audit.png

### Form interaction
ff-rdp click "input[name=email]"
ff-rdp type "input[name=email]" "user@example.com"
ff-rdp type "input[name=password]" "secret" --clear
ff-rdp click "button[type=submit]"
ff-rdp wait --text "Dashboard" --wait-timeout 10000

### Monitor and debug
ff-rdp console --follow --level error &
ff-rdp navigate https://example.com
ff-rdp console --level error
"##;

pub fn run(cli: &Cli) -> Result<(), AppError> {
    let results = json!(LLM_REFERENCE.trim());
    let meta = json!({"source": "static"});
    let envelope = output::envelope(&results, 1, &meta);

    OutputPipeline::from_cli(cli)?
        .finalize(&envelope)
        .map_err(AppError::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn llm_reference_contains_all_commands() {
        let commands = [
            "tabs",
            "navigate",
            "eval",
            "page-text",
            "dom",
            "dom stats",
            "dom tree",
            "computed",
            "styles",
            "console",
            "network",
            "perf",
            "screenshot",
            "snapshot",
            "geometry",
            "responsive",
            "click",
            "type",
            "wait",
            "cookies",
            "storage",
            "reload",
            "back",
            "forward",
            "inspect",
            "sources",
            "launch",
            "scroll",
        ];
        let subcommands = [
            "perf compare",
            "perf audit",
            "perf vitals",
            "perf summary",
            "a11y contrast",
            "scroll to",
            "scroll by",
            "scroll container",
            "scroll until",
            "scroll text",
        ];
        for cmd in subcommands {
            assert!(
                LLM_REFERENCE.contains(cmd),
                "LLM reference missing subcommand: {cmd}"
            );
        }
        for cmd in commands {
            assert!(
                LLM_REFERENCE.contains(&format!("### {cmd}")),
                "LLM reference missing command: {cmd}"
            );
        }
    }
}
