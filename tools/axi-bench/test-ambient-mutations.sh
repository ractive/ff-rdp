#!/bin/bash
set -eu
root="$(cd "$(dirname "$0")" && pwd -P)"
scratch="${FF_RDP_BENCH_AMBIENT_MUTATION_OUTPUT:-$(mktemp -d -t ff-rdp-ambient-mutations-XXXXXX)}"
mkdir -p "$scratch"
echo "Ambient mutation evidence: $scratch"
for mutation in baseline-leak judge-leak missing-settings lost-failure lost-confirmation; do
  awk -v mutation="$mutation" '
    mutation == "baseline-leak" && /^treatment="/ {
      $0="treatment=session-start"; changed++
    }
    mutation == "judge-leak" && /^if \[ "\$judge" = 0 \] && \[ "\$treatment" = session-start \]; then/ {
      $0="if [ \"$treatment\" = session-start ]; then"; changed++
    }
    mutation == "missing-settings" && /args\+=\(--settings/ { $0=":"; changed++ }
    /\|\| status=65/ { status_guards++ }
    mutation == "lost-failure" && status_guards == 1 && /\|\| status=65/ { sub(/status=65/, "status=0"); changed++ }
    mutation == "lost-confirmation" && status_guards == 2 && /\|\| status=65/ { sub(/status=65/, "status=0"); changed++ }
    { print }
    END { if (changed != 1) exit 2 }
  ' "$root/claude-record.sh" > "$scratch/$mutation.sh"
  chmod +x "$scratch/$mutation.sh"
  set +e
  FF_RDP_BENCH_TEST_RECORDER="$scratch/$mutation.sh" bash "$root/test-ambient.sh" > "$scratch/$mutation.log" 2>&1
  status=$?
  set -e
  printf '%s\n' "$status" > "$scratch/$mutation.exit"
  [ "$status" != 0 ] || { echo "Mutation survived: $mutation" >&2; exit 1; }
  case "$mutation" in
    baseline-leak|judge-leak) diagnostic='Baseline/judge isolation violated';;
    missing-settings|lost-failure|lost-confirmation) diagnostic='Delivery failure status lost';;
  esac
  rg -q "$diagnostic" "$scratch/$mutation.log" || { cat "$scratch/$mutation.log"; exit 1; }
  echo "PASS mutation $mutation rejected by intended assertion"
done

for mutation in help-as-browser invented-verbs compound-as-browser; do
  awk -v mutation="$mutation" '
    mutation == "help-as-browser" && /elif \(\$args \| help_only\)/ {
      print "    elif ($args | help_only) then (if ($args | length) > 1 then \"browser command\" else \"--help\" end)"; changed++; next
    }
    mutation == "invented-verbs" && /"styles", "cascade", "scroll", "home"/ {
      sub(/"home"/, "\"home\", \"fill\", \"press\", \"page\""); changed++
    }
    mutation == "compound-as-browser" && /if \(test\(/ {
      print "  if false then \"other\""; changed++; next
    }
    { print }
    END { if (changed != 1) exit 2 }
  ' "$root/first-tool.jq" > "$scratch/$mutation.jq"
  set +e
  FF_RDP_BENCH_TEST_CLASSIFIER="$scratch/$mutation.jq" bash "$root/test-ambient.sh" > "$scratch/$mutation.log" 2>&1
  status=$?
  set -e
  printf '%s\n' "$status" > "$scratch/$mutation.exit"
  [ "$status" != 0 ] || { echo "Mutation survived: $mutation" >&2; exit 1; }
  rg -q 'Classifier regression:' "$scratch/$mutation.log" || { cat "$scratch/$mutation.log"; exit 1; }
  echo "PASS mutation $mutation rejected by intended assertion"
done
