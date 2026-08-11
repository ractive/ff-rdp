export const meta = {
  name: 'new-ralph-loop',
  description: 'Sequential iteration runner: implement → review/merge → verify per iteration, stop on first failure',
  phases: [{ title: 'Run', detail: 'one implement + one review/merge (+ ci-wait, + verify) agent per iteration' }],
}

// Engine for the /new-ralph-loop skill. Invoked as:
//   Workflow({ scriptPath: '<skill>/scripts/ralph.workflow.js', args: {...} })
// args shape (see SKILL.md — args.preflight is preflight.sh --json stdout, verbatim):
//   {
//     preflight: { repo_root, cache_dir, plan_prefix, branch_prefix, range,
//                  iterations: [{ n, status, plan_path, title, missing? }] },
//     copilot: false,
//     models: { implement: 'opus', review: 'sonnet', verify: 'haiku' },
//     skillDir: '/Users/james/.claude/skills/new-ralph-loop',
//     agentCaps: { agentTool: false, sendMessage: true, agentType: null },  // smoke-probed 2026-07-04
//     skipMissing: false,
//     startedAt: '<date -u +%FT%TZ>',   // Date.now() is unavailable in workflow scripts
//   }
// Git (merge commits on origin/main) is the only durable ledger. Stop on first failure.
//
// NOTE: workflow scripts cannot run shell commands — anything needing git/gh
// (verification, CI polling) must go through an agent(). That is why verify is
// a tiny haiku agent and CI waiting is delegated, not polled here.

// Claude Code 2.1.197 delivers args as a JSON STRING (verified 2026-07-04 via
// args-probe), despite the tool doc saying objects pass verbatim. Parse
// defensively so this keeps working when/if the harness starts passing objects.
const A = typeof args === 'string' ? JSON.parse(args) : (args || {})

const pf = A.preflight
if (!pf || !Array.isArray(pf.iterations)) {
  throw new Error('args.preflight missing or malformed — pass preflight.sh --json output verbatim as args.preflight')
}
const copilot = A.copilot === true
const models = Object.assign({ implement: 'opus', review: 'sonnet', verify: 'haiku' }, A.models || {})
const skillDir = A.skillDir || '/Users/james/.claude/skills/new-ralph-loop'
const caps = Object.assign({ agentTool: false, sendMessage: true, agentType: null }, A.agentCaps || {})
const skipMissing = A.skipMissing === true
const startedAt = A.startedAt || null

// ---------------------------------------------------------------- schemas

const IMPLEMENT_SCHEMA = {
  type: 'object',
  required: ['ok', 'branch'],
  properties: {
    ok: { type: 'boolean', description: 'true only if the plan is implemented, gates are green, and the PR exists' },
    branch: { type: 'string', description: 'the iteration branch, e.g. iter-16/events-feature' },
    pr_number: { type: ['integer', 'null'] },
    pr_url: { type: ['string', 'null'] },
    quality_gates: { enum: ['passed', 'failed', 'skipped'], description: 'skipped = no gates defined in CLAUDE.md/README' },
    xtask_ready: { enum: ['passed', 'no-xtask'] },
    failure_reason: { type: ['string', 'null'], description: 'REQUIRED when ok=false: "<category>: <one precise paragraph>"' },
    notes: { type: ['string', 'null'], description: 'at most 2 sentences; shown in the final ledger' },
  },
}

const REVIEW_SCHEMA = {
  type: 'object',
  required: ['ok', 'merged'],
  properties: {
    ok: { type: 'boolean', description: 'review + discipline + plan-update all completed' },
    merged: { type: 'boolean', description: 'true only if /merge-pr actually merged and pushed main' },
    blocked_on_ci: { type: 'boolean', description: 'true when ALL review work is complete and ONLY still-pending PR checks prevent the merge — this is a hand-off, not a failure' },
    head_sha: { type: ['string', 'null'], description: 'git rev-parse HEAD of the PR branch — REQUIRED when blocked_on_ci=true' },
    pr_number: { type: ['integer', 'null'] },
    merge_commit: { type: ['string', 'null'], description: 'merge SHA if /merge-pr reported it' },
    findings_fixed: { type: ['integer', 'null'], description: 'count of review findings fixed' },
    discipline: { enum: ['passed', 'fixed-then-passed', 'failed'] },
    next_plan_adapted: { type: 'boolean' },
    failure_reason: { type: ['string', 'null'], description: 'REQUIRED when ok=false, or when merged=false and blocked_on_ci is not true' },
  },
}

const CI_MERGE_SCHEMA = {
  type: 'object',
  required: ['merged', 'checks_state'],
  properties: {
    merged: { type: 'boolean' },
    checks_state: { enum: ['passed', 'failed', 'pending'] },
    merge_commit: { type: ['string', 'null'] },
    failure_reason: { type: ['string', 'null'], description: 'REQUIRED when checks_state=failed: quote the failing check(s)' },
  },
}

const VERIFY_SCHEMA = {
  type: 'object',
  required: ['merge_found'],
  properties: {
    merge_found: { type: 'boolean' },
    merge_commit: { type: ['string', 'null'], description: 'full SHA from git rev-parse' },
    log_line: { type: ['string', 'null'], description: 'the matched git log --oneline line' },
  },
}

// ---------------------------------------------------------------- prompts

const UNATTENDED = `You are running unattended inside an automated workflow. There is NO human available.
If any skill, gate, or instruction says "stop and tell the user" or "do not proceed until the user
confirms" (e.g. /create-pr's test-coverage gate, /merge-pr's pre-flight checks), you MUST NOT stop
or wait. Instead: resolve the issue autonomously when safely possible (write the missing tests,
merge main into the branch, fix the conflict, re-run the gates), or — if genuinely unresolvable —
abandon the remaining steps and return your final structured result with ok=false and a precise
failure_reason. Never call AskUserQuestion. Never idle waiting for input. Never force-push.`

function milestoneHow(cap) {
  return cap.sendMessage
    ? `send a one-line SendMessage to "main" (if SendMessage is a deferred tool, load it first with ToolSearch "select:SendMessage")`
    : `append a single line "$(date -u +%FT%TZ) <text>" to ${pf.cache_dir}/progress.log`
}

function implementPrompt(it) {
  const delegation = caps.agentTool
    ? `Use an analyst/worker split: do the analysis YOURSELF — read the plan and relevant docs, make every design and API decision, decompose into well-specified mechanical tasks — then delegate code-writing to subagents via the Agent tool, giving each a distilled brief (exact file paths, decisions already made, acceptance criteria). Review and integrate each worker's diff yourself.`
    : `Work in a single context (no subagent delegation is available in this environment). Keep reads targeted — the plan plus directly relevant files — to control token use.`
  const copilotStep = copilot
    ? `9) Immediately after the PR exists, kick off the GitHub Copilot review: gh pr edit <PR-number> --add-reviewer @copilot — fire-and-forget, do NOT wait for it.`
    : `9) Copilot is disabled for this run: do NOT add @copilot as a reviewer.`
  return `${UNATTENDED}

You are implementing iteration ${it.n} of this project${it.title ? ` — "${it.title}"` : ''}. Steps, in order:
1) cd ${pf.repo_root} and sync: git fetch origin && git switch main && git pull --ff-only origin main. Then create a new branch ${pf.branch_prefix}-${it.n}/<short-description> from main.
2) Read the iteration plan at ${it.plan_path}. If it is not at that path, locate it with the glob **/${pf.plan_prefix}-${it.n}-*.md (hyalo find --glob if available, else find). If it cannot be found, return ok=false with failure_reason "plan-missing: ...".
3) Milestone (${milestoneHow(caps)}): "iter-${it.n} implementing: <plan title> — <2-sentence gist of what this iteration delivers> — approach: <1 sentence>". Send a milestone at each of steps 3, 6, and 10 (up to 4 total — a fourth only if a gate failure forces a notable detour). This phase can run 30–90 min; those checkpoints are the user's only window into it, so make every one concrete — name files, counts, and decisions, never "still working".
4) Implement everything in the plan: code, tests, error handling. ${delegation}
5) Update all docs, help texts, and project documentation affected by the change.
6) Before running gates, send a milestone: "iter-${it.n} implemented — <N> files changed, +<ins>/−<del> (from git diff --stat main...HEAD); running quality gates". Then run the project quality gates (the specific commands are in CLAUDE.md; if there is no CLAUDE.md, use the repo README). Fix failures.
7) If the repo has a crates/xtask crate: list its actual subcommands (cargo run -p xtask -- --help), run every check-* gate it offers (passing --plan ${it.plan_path} / --base origin/main only where that gate's own --help documents such flags), and fix every reported failure — do not proceed until they all exit 0. Do NOT invent subcommand names not in the help output. No xtask crate → skip this step.
8) Invoke the /create-pr skill (via the Skill tool). Its gates fall under the unattended rule above: satisfy them autonomously (e.g. write missing tests) — never wait for a user. If a PR already exists for this branch (retry scenario), /create-pr updates it; that is fine.
${copilotStep}
10) Milestone: "iter-${it.n} PR #<number> created: <url> — <N> files, +<ins>/−<del>; <one-line headline of the main changes>".
11) Finish by returning ONLY the structured result. Do NOT write any sentinel or done file — your structured return IS the completion signal.`
}

function reviewPrompt(it, next, impl) {
  const prRef = impl.pr_number
    ? `The PR is #${impl.pr_number}.`
    : `Find the PR number with: gh pr view --json number (on the branch).`
  const reviewStep = copilot
    ? `Invoke the /review-pr skill and fix all review issues.`
    : `Invoke the /review-pr skill with argument "local-only" (skip every Copilot step: do not add @copilot as reviewer, do not poll for Copilot findings) and fix all review issues.`
  const nextStep = next && next.plan_path
    ? `4) Next-plan adaptation: the next iteration in this run is ${next.n} with its plan at ${next.plan_path}. Based on what you learned this iteration, adapt its scope if needed and commit the edit onto this PR branch. IMPORTANT: edit the file in place — do NOT rename or move it; the running workflow references this exact path.`
    : `4) This is the last iteration of the run — no next-plan adaptation.`
  return `${UNATTENDED}

You are reviewing and merging iteration ${it.n}. A PR has already been created. ${prRef} Steps, in order:
1) cd ${pf.repo_root} && git switch ${impl.branch}. Then run the discipline checks and fix what they find:
   a) ${skillDir}/scripts/claims-vs-code.sh --branch ${impl.branch} --base main > ${pf.cache_dir}/iter-${it.n}-claims.md (advisory). Append the report to the PR body: gh pr edit <PR> --body "$(gh pr view <PR> --json body -q .body)$(printf '\\n\\n')$(cat ${pf.cache_dir}/iter-${it.n}-claims.md)".
   b) ${skillDir}/scripts/ac-fidelity-check.sh --plan ${it.plan_path} --branch ${impl.branch} --base main (hard gate). For each failure: implement the missing test/code so the AC matches the diff, soften the AC text to match what actually shipped, or annotate it with [deferred — new plan: <path>] and create that follow-up plan.
   c) If the repo has crates/xtask: list its subcommands (cargo run -p xtask -- --help) and do not proceed until every check-* gate it actually offers exits 0. Do NOT invent subcommand names not in the help output.
2) Milestone (${milestoneHow(caps)}): "iter-${it.n} review: running /review-pr on PR #<number>". ${reviewStep} Make the judgment calls yourself; commit and push fixes onto the PR branch.
3) Update the plan at ${it.plan_path}: tick every "- [ ]" scope checkbox whose work actually landed in this PR (verify against the real diff — never tick speculatively), update each section heading's [N/M] count, and leave genuinely incomplete boxes unchecked with a short note. Commit onto the PR branch and push.
${nextStep}
5) Invoke the /merge-pr skill (via the Skill tool). Its pre-flight gates (dirty tree, unpushed commits, behind main, conflicts) fall under the unattended rule: fix the condition (commit, push, merge main into the branch, resolve conflicts) and retry, or fail cleanly with merged=false and a precise failure_reason.
   PENDING PR CHECKS ARE A SPECIAL CASE — never report them as a failure:
   - If the merge is blocked only because PR checks are still running, wait for them: run gh pr checks <PR> --watch (re-run the command if it hits a tool timeout) for up to ~20 minutes total, then retry /merge-pr.
   - If a check FAILS: fix the cause and push if you can, then wait again; if unfixable, return merged=false with the failing check quoted in failure_reason.
   - If checks are STILL pending when your ~20 minutes are up: return ok=true, merged=false, blocked_on_ci=true, head_sha=$(git rev-parse HEAD), pr_number set. The workflow hands the merge off — do NOT keep waiting past that and do NOT let a forced cutoff catch you mid-wait; return the structured result with time to spare.
   Once /merge-pr's pre-flight passes it merges into main, pushes, and deletes the branch automatically — do not redo its cleanup.
6) Milestone (${milestoneHow(caps)}): "iter-${it.n} merged: PR #<number> (<findings_fixed> review fixes, merge <sha>)" — or "iter-${it.n} waiting on CI: PR #<number>" — or, if blocked, "iter-${it.n} BLOCKED: <reason>". Up to 3 milestones this phase (the review-start one from step 2, this one, and at most one more if the merge takes a notable detour).
7) Return ONLY the structured result. "merged" must reflect what /merge-pr actually did, not what you intended. No sentinel files.`
}

function ciWaitPrompt(it, prNumber, branch) {
  const prRef = prNumber ? `#${prNumber}` : `for branch ${branch} (find it: gh pr view --json number)`
  return `${UNATTENDED}

Iteration ${it.n}'s review is COMPLETE. PR ${prRef} (branch ${branch}) is ready to merge; only its CI checks were still running. Steps:
1) cd ${pf.repo_root}. Wait for the checks: run gh pr checks ${prNumber || ''} --watch (re-run the command if it hits a tool timeout) for up to ~25 minutes total.
2) If all checks pass: invoke the /merge-pr skill (unattended rules apply — fix trivial pre-flight issues like being behind main, never wait for a user). Return merged=true, checks_state="passed", and the merge SHA if reported.
3) If any check FAILS: do NOT merge and do NOT try to fix code — return merged=false, checks_state="failed", failure_reason quoting the failing check(s).
4) If checks are still pending when your time is up: return merged=false, checks_state="pending". Return the structured result with time to spare — do not let a forced cutoff catch you mid-wait.
Return ONLY the structured result.`
}

function verifyPrompt(it) {
  const legacy = pf.branch_prefix === 'iter'
    ? ` Also accept the legacy spelling "iteration-${it.n}/".`
    : ''
  return `You are a read-only verifier. Run exactly these commands and nothing else:
1) git -C ${pf.repo_root} fetch origin main
2) git -C ${pf.repo_root} log origin/main --merges --oneline -30
Look for a merge commit whose subject references the branch "${pf.branch_prefix}-${it.n}/" — the trailing slash is required, so "${pf.branch_prefix}-${it.n}b/" must NOT match.${legacy}
If found: get the full SHA with git -C ${pf.repo_root} rev-parse <short-sha>.
Do not modify anything. Return ONLY the structured result.`
}

// ---------------------------------------------------------------- main loop

const results = []
// resume_hint values (see SKILL.md "Completion handling"):
//   'retry-iteration'            — nothing durable landed; the generic resume command redoes it, which is correct
//   'merge-then-resume'          — review done, PR finished, merge blocked on CI: merge the PR manually when
//                                  green, then the SAME resume command is safe (preflight will see the merge and skip)
//   'finish-manually-or-retry'   — partial review work on an existing PR: either finish review+merge by hand then
//                                  resume (preflight skips it), or accept that resuming redoes the implement phase
//   'check-throttle'             — agent terminated: diagnose throttle vs crash from the API detail; for the review
//                                  phase ALSO re-check git first, the merge may have landed
//   'verify-manually'            — review claimed merged but verify found no merge commit: inspect by hand
const stop = (reason, it, phaseName, detail, resumeHint) => ({
  status: 'stopped',
  reason,
  stopped_at: { n: it.n, phase: phaseName },
  detail,
  resume_hint: resumeHint || 'retry-iteration',
  results,
  next_pending: it.n,
  range: pf.range,
  startedAt,
})

phase('Run')

for (let i = 0; i < pf.iterations.length; i++) {
  const it = pf.iterations[i]
  const next = pf.iterations[i + 1] || null

  if (it.status !== 'pending') {
    log(`iter-${it.n}: already ${it.status} — skipping`)
    results.push({ n: it.n, outcome: 'skipped-preflight' })
    continue
  }
  if (it.missing || !it.plan_path) {
    if (skipMissing) {
      log(`iter-${it.n}: plan missing — skipped by user choice`)
      results.push({ n: it.n, outcome: 'skipped-missing' })
      continue
    }
    return stop('plan-missing', it, 'preflight', `no plan file for ${pf.plan_prefix}-${it.n}`, 'retry-iteration')
  }

  // ---- implement ----
  log(`iter-${it.n}: implement starting — ${it.title || it.plan_path}`)
  const impl = await agent(implementPrompt(it), {
    label: `iter-${it.n} implement`,
    phase: 'Run',
    schema: IMPLEMENT_SCHEMA,
    model: models.implement,
    ...(caps.agentType ? { agentType: caps.agentType } : {}),
  })
  if (impl === null) {
    results.push({ n: it.n, outcome: 'terminated', phase: 'implement' })
    return stop('agent-terminated', it, 'implement',
      'implement agent() returned null (killed/skipped/terminal API error) — check the run view detail to distinguish throttle from crash',
      'check-throttle')
  }
  if (!impl.ok) {
    results.push({ n: it.n, outcome: 'implement-failed', branch: impl.branch || null, pr_number: impl.pr_number ?? null })
    return stop('implement-failed', it, 'implement', impl.failure_reason || 'no failure_reason provided', 'retry-iteration')
  }

  // ---- review + merge ----
  log(`iter-${it.n}: PR ${impl.pr_number ? '#' + impl.pr_number : '(number unknown)'} on ${impl.branch}`)
  const rev = await agent(reviewPrompt(it, next, impl), {
    label: `iter-${it.n} review+merge`,
    phase: 'Run',
    schema: REVIEW_SCHEMA,
    model: models.review,
    ...(caps.agentType ? { agentType: caps.agentType } : {}),
  })
  if (rev === null) {
    results.push({
      n: it.n, outcome: 'review-incomplete', pr_number: impl.pr_number ?? null, branch: impl.branch,
      note: 'review agent terminated — the merge may have landed; re-check git before acting',
    })
    return stop('agent-terminated', it, 'review',
      'review agent() returned null — NOTE: the merge may have landed before the agent died; the session MUST re-check git before declaring this iteration failed',
      'check-throttle')
  }

  const prNum = rev.pr_number ?? impl.pr_number ?? null
  let merged = rev.ok && rev.merged
  let mergeCommitClaim = rev.merge_commit || null

  // Review finished but the merge was blocked ONLY by still-running PR checks:
  // hand off to a small ci-wait+merge agent instead of failing the run.
  if (!merged && rev.ok && rev.blocked_on_ci) {
    log(`iter-${it.n}: review complete, merge waiting on CI (PR ${prNum ? '#' + prNum : '?'}, head ${rev.head_sha || '?'})`)
    for (let attempt = 1; attempt <= 2 && !merged; attempt++) {
      const cw = await agent(ciWaitPrompt(it, prNum, impl.branch), {
        label: `iter-${it.n} ci-wait+merge (${attempt}/2)`,
        phase: 'Run',
        schema: CI_MERGE_SCHEMA,
        model: models.review,
        effort: 'low',
      })
      if (cw === null) break
      if (cw.merged) { merged = true; mergeCommitClaim = cw.merge_commit || mergeCommitClaim; break }
      if (cw.checks_state === 'failed') {
        results.push({ n: it.n, outcome: 'review-incomplete', pr_number: prNum, branch: impl.branch, head_sha: rev.head_sha || null })
        return stop('review-failed', it, 'review',
          'PR checks failed after review completed: ' + (cw.failure_reason || 'see the PR checks tab'),
          'finish-manually-or-retry')
      }
      log(`iter-${it.n}: checks still pending after wait attempt ${attempt}/2`)
    }
    if (!merged) {
      results.push({ n: it.n, outcome: 'blocked-on-ci', pr_number: prNum, branch: impl.branch, head_sha: rev.head_sha || null })
      return stop('ci-blocked', it, 'review',
        `review is COMPLETE; only pending/stuck PR checks block the merge (PR ${prNum ? '#' + prNum : '?'}, head ${rev.head_sha || '?'}). Merge the finished PR when checks are green — do NOT re-run the iteration — then the normal resume command is safe (preflight will see the merge and skip this iteration).`,
        'merge-then-resume')
    }
  } else if (!merged) {
    results.push({ n: it.n, outcome: 'review-incomplete', pr_number: prNum, branch: impl.branch })
    return stop('review-failed', it, 'review', rev.failure_reason || 'no failure_reason provided', 'finish-manually-or-retry')
  }

  // ---- verify (cheap, best-effort) ----
  // Workflow scripts can't run git themselves, so this is a tiny haiku agent.
  // Its death must never sink a merged iteration: on null we record
  // merged-unverified and CONTINUE — if the run is truly out of budget, the
  // next implement agent dies immediately and next_pending lands correctly.
  const ver = await agent(verifyPrompt(it), {
    label: `iter-${it.n} verify`,
    phase: 'Run',
    schema: VERIFY_SCHEMA,
    model: models.verify,
    effort: 'low',
  })
  if (ver === null) {
    log(`iter-${it.n}: verify agent terminated — recording merge as unverified and continuing (session should spot-check git)`)
    results.push({
      n: it.n, outcome: 'merged-unverified', pr_number: prNum, branch: impl.branch,
      merge_commit: mergeCommitClaim, notes: impl.notes || null,
    })
    continue
  }
  if (!ver.merge_found) {
    results.push({ n: it.n, outcome: 'merge-unverified', pr_number: prNum, branch: impl.branch })
    return stop('merge-not-verified', it, 'verify',
      `review agent reported merged=true but no merge commit for ${pf.branch_prefix}-${it.n}/ found on origin/main`,
      'verify-manually')
  }

  log(`iter-${it.n}: merged — ${ver.merge_commit}`)
  results.push({
    n: it.n,
    outcome: 'merged',
    pr_number: prNum,
    branch: impl.branch,
    merge_commit: ver.merge_commit,
    notes: impl.notes || null,
  })
}

return {
  status: 'complete',
  reason: null,
  stopped_at: null,
  detail: null,
  resume_hint: null,
  results,
  next_pending: null,
  range: pf.range,
  startedAt,
}
