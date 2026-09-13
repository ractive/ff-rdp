# These are CLI names, not synonyms for browser actions (args.rs Command).
# Lifecycle, installation and script-management commands require adjudication.
def browser_commands:
  ["tabs", "navigate", "eval", "page-text", "dom", "console", "network",
   "perf", "screenshot", "click", "type", "wait", "cookies", "storage",
   "a11y", "reload", "back", "forward", "inspect", "sources", "snapshot",
   "geometry", "responsive", "emulate", "throttle", "manifest", "computed",
   "styles", "cascade", "scroll", "home"];
def command_paths:
  browser_commands + ["daemon", "launch", "install-skill", "install-hook",
    "skill-doc", "doctor", "profiles", "run", "record", "index", "consent",
    "completions", "dom stats", "dom tree", "perf vitals", "perf summary",
    "perf audit", "perf compare", "a11y contrast", "a11y summary",
    "scroll to", "scroll by", "scroll top", "scroll bottom", "scroll container",
    "scroll until", "scroll text", "daemon status", "daemon stop",
    "profiles list", "profiles prune", "record start", "record stop",
    "record status", "consent accept"];
def known_path: join(" ") as $path | $path == "" or (command_paths | index($path)) != null;
def help_only:
  if .[0] == "help" then .[1:] | known_path
  elif .[-1] == "--help" or .[-1] == "-h" then .[:-1] | known_path
  elif .[1] == "help" and (.[0] as $parent |
    ["dom", "perf", "a11y", "scroll", "daemon", "profiles", "record", "consent"] | index($parent)) != null
    then [.[0]] + .[2:] | known_path
  else false end;
def browser_invocation:
  . as $args |
  (browser_commands | index($args[0])) != null and
  # No help/version/option-terminator token can produce a browser-first claim.
  (any(.[]; . == "help" or . == "--help" or . == "-h" or
    . == "--version" or . == "-V" or . == "--") | not) and
  (if .[0] == "scroll" then
     (["to", "by", "top", "bottom", "container", "until", "text"] | index($args[1])) != null
   elif (.[0] == "a11y" or .[0] == "perf") and length > 1 and (.[1] | startswith("-") | not) then
     .[:2] | known_path
   else true end);
def classify_command:
  # A deliberately small lexical subset, not a shell parser. Quotes, expansion,
  # redirection, comments, compounds and even embedded newlines need raw review.
  if (test("\\A[ \\t]*ff-rdp(?:[ \\t]+[A-Za-z0-9_./:@%+=,-]+)*[ \\t]*\\z") | not) then "other"
  else [scan("[^ \\t]+")][1:] as $args |
    if $args == [] then "bare ff-rdp"
    elif ($args | help_only) then "--help"
    elif ($args | browser_invocation) then "browser command"
    else "other" end
  end;

# Only completed, top-level assistant tool_use blocks are agent decisions.
# Hook/system events, stream deltas and nested-agent tools never precede them.
[.[] | select(.type == "assistant" and .parent_tool_use_id == null) |
  .message.content[]? | select(.type == "tool_use")][0] as $first |
($first.input.command // null) as $command |
{
  id: ($first.id // null), name: ($first.name // null), command: $command,
  category: (if $first == null then "missing"
    elif $first.name != "Bash" or ($command | type) != "string" then "other"
    else $command | classify_command end),
  terminal_result: any(.[]; .type == "result"),
  result_error: any(.[]; .type == "result" and .is_error == true)
}
