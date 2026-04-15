---
title: "Dogfooding Eval Framework: ff-rdp vs Chrome MCP"
date: 2026-04-09
type: design
tags: [dogfooding, eval, benchmark, comparison, chrome-mcp]
status: draft
---

# Dogfooding Eval Framework: ff-rdp vs Chrome MCP

## The Discoverability Problem

A "playbook" (step-by-step guide) is the wrong abstraction. In real life, an LLM using ff-rdp doesn't have a playbook -- it has to discover what commands exist and compose them. The same LLM using Chrome MCP also has to discover tools.

**The real question**: Given a task like "analyze this website's performance", how well can an LLM accomplish it using ff-rdp vs Chrome MCP, **without a guide**?

This means we need to measure:
1. **Task completion**: Did the LLM extract the data? (binary pass/fail per assertion)
2. **Efficiency**: How many tool calls / tokens did it take?
3. **Reliability**: Did any commands error out?
4. **Discoverability**: Did the LLM find the right commands without hints?

## Eval Structure

### Test Cases

Each eval has:
- **Task prompt**: Natural language instruction (what a real user would say)
- **Target URL**: The page to analyze
- **Assertions**: Verifiable claims about what should be in the output
- **No hints**: The prompt does NOT mention specific commands or tools

Example:
```json
{
  "name": "perf-audit-comparis",
  "prompt": "Analyze the performance of this page and give me Core Web Vitals, resource loading summary, and the slowest requests.",
  "url": "https://www.comparis.ch/immobilien/result/list?...",
  "assertions": [
    "Output contains TTFB value in milliseconds",
    "Output contains FCP value",
    "Output contains CLS value",
    "Output contains total resource count",
    "Output identifies at least 3 slow requests with URLs",
    "Output identifies third-party domains"
  ]
}
```

### Task Categories

| Category | Example Prompt | Key Assertions |
|----------|---------------|----------------|
| Performance | "What are the Core Web Vitals?" | TTFB, FCP, LCP, CLS measured |
| Network | "How many third-party requests does this page make?" | Count, domains listed |
| Accessibility | "Check accessibility and contrast" | Contrast results, a11y tree |
| SEO | "Check the SEO basics" | Title, meta, OG, h1, canonical |
| Structure | "What framework is this built with?" | Framework detected, DOM stats |
| Interaction | "Find the search form and type 'Zurich'" | Input found, text entered |
| Navigation | "Go to comparis.ch, find apartments" | Page navigated, content found |
| Cookies | "What cookies does this site set?" | Cookie list with flags |
| Security | "Is this site served securely?" | HTTPS, cookie flags, headers |

### Runner Design

```
┌──────────────────────────────────────────────┐
│ Eval Runner                                  │
│                                              │
│  For each test case:                         │
│    1. Launch fresh browser (Firefox or Chrome)│
│    2. Navigate to URL                        │
│    3. Spawn LLM agent with task prompt       │
│       - ff-rdp agent: has ff-rdp CLI only    │
│       - chrome agent: has Chrome MCP only    │
│    4. Let agent work autonomously            │
│    5. Collect: output, tool calls, tokens    │
│    6. Grade: check assertions against output │
│    7. Score: pass/fail per assertion          │
│                                              │
│  Aggregate:                                  │
│    - Pass rate per category                  │
│    - Total tokens (efficiency)               │
│    - Tool calls count                        │
│    - Error rate                              │
│    - Time to completion                      │
└──────────────────────────────────────────────┘
```

### Grading

Binary pass/fail per assertion. A grader checks:
- Does the output contain the claimed data?
- Is the data plausible? (e.g., TTFB > 0 and < 30000)
- Did the agent complete without getting stuck in a loop?

### What This Measures

| Metric | What It Tells Us |
|--------|-----------------|
| **Pass rate** | Can the tool extract the data at all? |
| **Pass rate without hints** | Can an LLM discover how to use the tool? |
| **Token efficiency** | How expensive is it to use? (ff-rdp should be cheaper -- fewer tool calls) |
| **Error rate** | How reliable are the commands? |
| **Category gaps** | Where does ff-rdp need more commands? |

### Expected Results (Hypothesis)

| Category | ff-rdp Advantage | Chrome MCP Advantage |
|----------|-----------------|---------------------|
| Performance | High -- `perf audit` is one command | Low -- needs custom JS |
| Network | Medium -- `network` command exists | Low -- post-activation only |
| Accessibility | High -- `a11y` commands built-in | Low -- no built-in a11y |
| SEO | Medium -- `dom` queries work | Medium -- `find` is intuitive |
| Structure | High -- `snapshot`, `sources`, `dom stats` | Medium -- `read_page` |
| Interaction | Low -- CSS selectors required | High -- natural language `find` |
| Navigation | Equal | Equal |

### Discoverability Boost: The Skill Advantage

The **site-audit skill** ([[iterations/iteration-42-site-audit-skill]]) is ff-rdp's answer to discoverability. Without it, an LLM must:
1. Guess that `ff-rdp` exists
2. Run `ff-rdp --help` to see commands
3. Run `ff-rdp perf --help` to see subcommands
4. Compose the right flags

With the skill, the LLM gets a complete playbook injected into context. This is analogous to Chrome MCP's advantage of having tool schemas auto-loaded.

**Eval comparison should run both**:
- ff-rdp **without** skill (raw CLI discoverability)
- ff-rdp **with** skill (guided audit)
- Chrome MCP (baseline)

## Implementation Plan

### Phase 1: Manual Eval (Now)
Run the dogfooding sessions we already do, but structured:
- Same sites, same prompts, both tools
- Document pass/fail per assertion in the dogfooding report
- Track over time in a comparison table

### Phase 2: Semi-Automated (Iteration 42)
- Eval JSON files with test cases and assertions
- Shell script that runs both agents on same test cases
- Manual grading of outputs

### Phase 3: Fully Automated (Future)
- LLM-based grading (like skill-creator's grader agent)
- Automatic comparison reports
- Regression detection: if pass rate drops, flag it
- CI integration: run evals on each release
