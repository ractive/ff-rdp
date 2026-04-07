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

CONSOLE
  Error messages only:
    ff-rdp console --level error --jq '.results[].message'

GENERAL
  Count results from any command:
    ff-rdp <command> --jq '.total'

  Get just the first result:
    ff-rdp <command> --jq '.results[0]'

  Extract specific fields:
    ff-rdp perf --jq '[.results[] | {{url, duration_ms}}]'"#
    );
}
