export const meta = {
  name: 'nrl-smoke',
  description: 'Capability probe for new-ralph-loop: Agent/SendMessage/Skill availability inside workflow subagents',
  phases: [{ title: 'Probe', detail: 'one agent introspects its toolset and tests nested spawn + messaging' }],
}

// Probe the three capabilities the ralph.workflow.js design is contingent on:
//  (a) Agent tool  -> analyst/worker split possible inside the implement agent
//  (b) SendMessage -> milestone one-liners to the main conversation
//  (c) Skill tool  -> /create-pr, /review-pr, /merge-pr invocable by agents
// Re-run this after Claude Code upgrades if agent capabilities change:
//   Workflow({scriptPath: '<skill>/scripts/smoke.workflow.js'})            // default agent type
//   Workflow({scriptPath: '...', args: {agentType: 'claude'}})             // fallback probe

const SMOKE_SCHEMA = {
  type: 'object',
  required: ['tools', 'agent_tool', 'send_message', 'skill_tool'],
  properties: {
    tools: { type: 'array', items: { type: 'string' }, description: 'names of all tools available to you' },
    agent_tool: {
      type: 'object',
      required: ['available', 'spawn_ok'],
      properties: {
        available: { type: 'boolean' },
        spawn_ok: { type: 'boolean', description: 'true only if the nested haiku subagent actually returned pong' },
        detail: { type: 'string' },
      },
    },
    send_message: {
      type: 'object',
      required: ['available', 'sent_ok'],
      properties: {
        available: { type: 'boolean' },
        sent_ok: { type: 'boolean', description: 'true only if the SendMessage call to "main" succeeded' },
        detail: { type: 'string' },
      },
    },
    skill_tool: {
      type: 'object',
      required: ['available'],
      properties: {
        available: { type: 'boolean', description: 'is a Skill tool present in your toolset' },
        skills_listed: { type: 'array', items: { type: 'string' }, description: 'a few skill names you can see, if any' },
      },
    },
  },
}

// args may arrive as a JSON string (Claude Code 2.1.197) — parse defensively.
const A = typeof args === 'string' ? JSON.parse(args) : (args || {})

const opts = {
  label: 'smoke-probe' + (A.agentType ? `-${A.agentType}` : ''),
  phase: 'Probe',
  model: 'sonnet',
  effort: 'low',
  schema: SMOKE_SCHEMA,
  ...(A.agentType ? { agentType: A.agentType } : {}),
}
log('probe agentType: ' + (A.agentType || '(default)'))

const r = await agent(
  `You are a capability probe. Do exactly these three checks and nothing else — do not read files, do not invoke any skill, do not run shell commands except where stated:

A) List the names of all tools currently available to you. Note specifically whether "Agent" (or "Task"), "SendMessage", and "Skill" appear.

B) If an Agent/Task tool is available: spawn exactly ONE subagent with model haiku and the prompt "Reply with the single word: pong". Report whether it actually returned pong. If the tool is unavailable or the spawn errors, report available/spawn_ok accordingly with the error text in detail.

C) If a SendMessage tool is available: send exactly the message "nrl-smoke: hello from a workflow child" to the recipient "main". Report whether the call succeeded, with any error text in detail.

For the Skill check, only report whether the tool exists and (if visible) a few example skill names — do NOT invoke any skill.

Then return the structured result. Your final output must be only the JSON object.`,
  opts,
)

log('smoke result: ' + JSON.stringify(r))
return r
