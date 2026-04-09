pub fn run() {
    println!(
        r#"ff-rdp recipes — curated --jq one-liners for common tasks

PERFORMANCE
  Slowest 5 resources by duration:
    ff-rdp perf --jq '[.results | sort_by(-.duration_ms) | limit(5;.) | {{url,duration_ms}}]'

  Total transfer size of all resources:
    ff-rdp perf --all --jq '[.results[].transfer_size] | add'

  Third-party resource count:
    ff-rdp perf --all --jq '[.results[] | select(.third_party)] | length'

  Cached resource URLs:
    ff-rdp perf --all --jq '[.results[] | select(.from_cache) | .url]'

  Resources by type:
    ff-rdp perf --all --jq '.results | group_by(.resource_type) | map({{type: .[0].resource_type, count: length}})'

WEB VITALS
  All vitals as name=value pairs:
    ff-rdp perf vitals --jq '.results | to_entries[] | "\(.key)=\(.value)"'

  Just the LCP value:
    ff-rdp perf vitals --jq '.results.lcp_ms'

DOM
  Count all DOM nodes:
    ff-rdp dom stats --jq '.results.node_count'

  Find images without lazy loading:
    ff-rdp dom stats --jq '.results.images_without_lazy'

NETWORK
  Failed requests (status >= 400):
    ff-rdp network --jq '[.results[] | select(.status >= 400) | {{url,status}}]'

  Total transfer by domain:
    ff-rdp perf --all --group-by domain --jq '.results'

  Stream network events live:
    ff-rdp network --follow
    ff-rdp network --follow --filter ".js"

CONSOLE
  Error messages only:
    ff-rdp console --level error --jq '.results[].message'

  Tail console output live:
    ff-rdp console --follow
    ff-rdp console --follow --level error

PAGE UNDERSTANDING
  Snapshot page structure for LLM:
    ff-rdp snapshot

  Shallow snapshot (depth 3):
    ff-rdp snapshot --depth 3

  Get screenshot as base64 for AI agents:
    ff-rdp screenshot --base64

  Check if elements overlap:
    ff-rdp geometry ".modal" ".overlay" --jq '.results.overlaps'

  Get viewport dimensions:
    ff-rdp geometry "body" --jq '.results.viewport'

  Find hidden elements:
    ff-rdp geometry "div" --jq '[.results.elements[] | select(.visible == false)]'

ACCESSIBILITY
  Full accessibility tree:
    ff-rdp a11y

  Interactive elements only (buttons, links, inputs):
    ff-rdp a11y --interactive

  Accessibility subtree for a section:
    ff-rdp a11y --selector ".main-content" --depth 4

  Extract all link names:
    ff-rdp a11y --jq '[.. | select(.role? == "link") | .name]'

  WCAG contrast check (all text elements):
    ff-rdp a11y contrast

  Show only contrast failures:
    ff-rdp a11y contrast --fail-only

  Contrast summary stats:
    ff-rdp a11y contrast --jq '.meta.summary'

GENERAL
  Count results from any command:
    ff-rdp <command> --jq '.total'

  Get just the first result:
    ff-rdp <command> --jq '.results[0]'

  Extract specific fields:
    ff-rdp perf --jq '[.results[] | {{url, duration_ms}}]'

INTERACTION WORKFLOWS
  Fill and submit a form:
    ff-rdp click "input[name=email]"
    ff-rdp type "input[name=email]" "user@example.com"
    ff-rdp type "input[name=password]" "secret" --clear
    ff-rdp click "button[type=submit]"
    ff-rdp wait --text "Dashboard" --wait-timeout 10000

  Wait for dynamic content:
    ff-rdp navigate https://example.com
    ff-rdp wait --selector ".content-loaded"
    ff-rdp dom ".content-loaded" --text

  Navigate and verify:
    ff-rdp navigate https://example.com --wait-text "Welcome"
    ff-rdp eval "document.title"
    ff-rdp dom "h1" --text

ERROR HANDLING
  Check if element exists before clicking:
    ff-rdp dom "button.submit" --count --jq '.results.count'
    ff-rdp click "button.submit"

  Retry on timeout:
    ff-rdp wait --selector ".loaded" --wait-timeout 10000
    # If timed out, increase timeout or check the page:
    ff-rdp snapshot --depth 2

  Verify navigation succeeded:
    ff-rdp navigate https://example.com --wait-text "Expected content"
    ff-rdp eval "document.readyState"
    ff-rdp eval "window.location.href"

CROSS-COMMAND WORKFLOWS
  Full page audit (navigate → perf → a11y → network → screenshot):
    ff-rdp navigate https://example.com --with-network --wait-text "loaded"
    ff-rdp perf audit
    ff-rdp a11y contrast --fail-only
    ff-rdp network --detail --limit 10
    ff-rdp screenshot -o audit.png

  Monitor console while testing:
    ff-rdp console --follow --level error &
    ff-rdp navigate https://example.com
    ff-rdp eval "triggerAction()"
    ff-rdp console --level error

  Compare before/after performance:
    ff-rdp perf compare https://old.example.com https://new.example.com --label "Before,After"

  Extract all links:
    ff-rdp dom "a[href]" --attrs --jq '[.results[].href]'"#
    );
}
