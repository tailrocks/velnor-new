# Subagent model policy

This document is the detailed, mandatory model and delegation policy referenced by [AGENTS.md](../../AGENTS.md).

## Subagent Model Configuration — Role-Based Exact Match, No Fallback

Every subagent must use an explicitly assigned model and reasoning configuration based on its role.

There are exactly two permitted configurations:

### Implementation / Execution

- **Model:** `gpt-6-luna`
- **Reasoning effort:** `max`

### Review / Verification

- **Model:** `gpt-6.1-sol`
- **Reasoning effort:** `medium`

These assignments are mandatory.

No other model or reasoning configuration is permitted.

---

## 1. Implementation Model Policy

All agents performing implementation, execution, investigation, research that directly supports implementation, debugging, fixing, coding, refactoring, migration, configuration changes, test creation, or other productive work must use exactly:

`gpt-6-luna` + `reasoning_effort=max`

This includes:

- implementation agents;
- coding agents;
- debugging agents;
- fixing agents;
- refactoring agents;
- migration agents;
- research agents supporting implementation;
- architecture/design agents producing implementation decisions;
- performance optimization agents making changes;
- security-remediation agents making changes;
- test-writing agents;
- integration agents;
- build/CI repair agents;
- repository cleanup agents;
- agents resolving blockers;
- agents preparing commits;
- agents performing operational execution;
- any descendant subagents performing implementation work.

For implementation work:

- Always specify exactly `gpt-6-luna`.
- Always specify exactly `max` reasoning effort.
- Never use `gpt-6.1-sol`.
- Never use any other Sol model.
- Never use Astra.
- Never use another Luna version.
- Never use automatic model selection.
- Never silently upgrade, downgrade, or substitute the model.

The only valid implementation configuration is:

`gpt-6-luna` + `max`

---

## 2. Review Model Policy

Every agent whose purpose is to review, audit, critique, verify, validate, challenge, cross-check, or independently assess work must use exactly:

`gpt-6.1-sol` + `reasoning_effort=medium`

This includes:

- code reviewers;
- implementation reviewers;
- architecture reviewers;
- security reviewers;
- performance reviewers;
- correctness reviewers;
- test reviewers;
- CI/CD reviewers;
- design reviewers;
- specification reviewers;
- independent verification agents;
- regression reviewers;
- final-quality reviewers;
- adversarial reviewers;
- agents checking whether requirements were actually satisfied;
- agents reviewing another subagent's conclusions or changes;
- agents performing final acceptance review;
- any descendant subagent performing review work.

For review work:

- Always specify exactly `gpt-6.1-sol`.
- Always specify exactly `medium` reasoning effort.
- Never use `gpt-6-luna` for reviews.
- Never use another Sol version.
- Never use Astra.
- Never use automatic model selection.
- Never silently upgrade, downgrade, or substitute the model.

The only valid review configuration is:

`gpt-6.1-sol` + `medium`

---

## 3. Strict Separation of Responsibilities

Maintain a strict separation between implementation and review.

**Implementation → `gpt-6-luna` + `max`**

**Review → `gpt-6.1-sol` + `medium`**

Do not allow an implementation agent to act as the sole reviewer of its own work.

Important implementation work should be independently reviewed by a separate `gpt-6.1-sol` + `medium` subagent.

When a reviewer discovers a problem requiring changes:

1. the `gpt-6.1-sol` reviewer identifies and explains the problem;
2. implementation/fixing is delegated back to a `gpt-6-luna` + `max` agent;
3. the resulting fix is reviewed again by `gpt-6.1-sol` + `medium`;
4. repeat this implementation → review loop until the reviewer finds no material outstanding issues.

A review agent should not silently switch into implementation mode.

An implementation agent should not be used as the independent final reviewer.

---

## 4. Mandatory Implementation → Review Loop

For substantive work, follow this execution pattern:

1. **Decompose** the goal into independent workstreams.
2. **Implement in parallel** using `gpt-6-luna` + `max`.
3. **Review independently in parallel** using `gpt-6.1-sol` + `medium`.
4. Send actionable review findings back to `gpt-6-luna` implementation agents.
5. Implement fixes with `gpt-6-luna` + `max`.
6. Re-review fixes with `gpt-6.1-sol` + `medium`.
7. Continue until all important review findings are resolved.
8. Perform a final independent review with `gpt-6.1-sol` + `medium`.
9. Integrate only work that passes the required verification.

Do not consider implementation complete merely because the implementing agent says it is complete.

Completion requires independent review.

---

## 5. No Other Models

The following are prohibited unless this policy is explicitly changed by the user:

- GPT-6 Astra;
- any Astra variant;
- GPT-6 Sol;
- GPT-6.1 Sol at reasoning levels other than `medium`;
- GPT-6 Luna at reasoning levels other than `max`;
- any GPT-5.x model;
- any other GPT model;
- any third-party model;
- automatic model selection;
- unspecified/default model selection;
- fallback routing;
- "best available" routing;
- "strongest available" routing;
- model substitution based on availability, cost, latency, capability, or runtime recommendation.

Do not interpret a newer, larger, stronger, or more capable model as an acceptable substitute.

Exact model identity is part of the execution requirement.

---

## 6. Fail-Closed Behavior

If an implementation subagent cannot be launched with exactly:

`gpt-6-luna` + `max`

then:

**do not launch that implementation task using another model.**

If a review subagent cannot be launched with exactly:

`gpt-6.1-sol` + `medium`

then:

**do not perform that review using another model.**

If the runtime:

- rejects the required model;
- does not expose the required model;
- does not support the required reasoning effort;
- rate-limits the model and proposes another model;
- automatically routes to another model;
- cannot guarantee which model will execute;
- cannot verify the selected model when verification is available;

treat the operation as non-compliant.

Preserve completed work and report the blocked operation.

Do not bypass the policy through fallback.

---

## 7. Verification of Model Compliance

Whenever the runtime exposes execution metadata, verify the actual model and reasoning effort used.

For implementation agents, verify:

- model = `gpt-6-luna`
- reasoning effort = `max`

For review agents, verify:

- model = `gpt-6.1-sol`
- reasoning effort = `medium`

If an implementation task ran with anything other than `gpt-6-luna` + `max`, treat that execution as non-compliant.

If a review ran with anything other than `gpt-6.1-sol` + `medium`, treat that review as non-compliant.

Do not count a non-compliant execution toward completion of the goal.

Repeat the work with the correct configuration if it becomes available.

---

## 8. Recursive Subagents

This policy applies recursively at every delegation depth.

If a subagent creates another subagent:

- implementation descendants must use `gpt-6-luna` + `max`;
- review descendants must use `gpt-6.1-sol` + `medium`.

A child agent must propagate this policy to every descendant it creates.

No nested agent may use automatic model selection or introduce another model.

---

## 9. Parallel Execution

Use subagents aggressively.

Parallelize all independent work that can safely run concurrently.

Implementation parallelism:

`gpt-6-luna` + `max`

Review parallelism:

`gpt-6.1-sol` + `medium`

A typical parallel structure should look like:

- multiple `gpt-6-luna` implementation/research agents working independently;
- multiple `gpt-6.1-sol` reviewers independently checking their outputs;
- targeted `gpt-6-luna` agents fixing identified issues;
- `gpt-6.1-sol` reviewers re-verifying those fixes.

Parallelism must never be achieved by introducing additional models.

---

## 10. Independent Review Requirement

Use `gpt-6.1-sol` + `medium` reviewers aggressively.

For high-impact changes, prefer multiple independent reviews covering different dimensions, such as:

- correctness;
- completeness;
- architecture;
- maintainability;
- security;
- performance;
- concurrency;
- edge cases;
- regression risk;
- tests;
- CI/CD;
- usability;
- adherence to the original goal.

Reviewers should actively challenge implementation-agent conclusions rather than merely confirm them.

They should search for:

- missing requirements;
- incomplete implementations;
- hidden regressions;
- incorrect assumptions;
- insufficient tests;
- unhandled edge cases;
- unnecessary complexity;
- dead code;
- duplicated functionality;
- performance regressions;
- security issues;
- architectural inconsistencies;
- claims that are not supported by evidence.

---

## 11. Parent-Agent Responsibility

The parent agent remains responsible for:

- decomposing the goal;
- determining which tasks are implementation versus review;
- assigning the correct model to every subagent;
- maximizing useful parallelism;
- preventing fallback;
- ensuring independent review;
- routing review findings back to implementation agents;
- resolving disagreements between agents;
- integrating compatible changes;
- ensuring tests and deterministic checks are executed;
- rejecting non-compliant agent executions;
- ensuring every required item is actually completed;
- ensuring final acceptance review succeeds.

The parent agent must not weaken the model policy merely to make progress faster.

---

## Mandatory Invariants

Maintain these invariants throughout the entire execution:

> **All implementation, execution, coding, fixing, debugging, and implementation-oriented research must use exclusively `gpt-6-luna` with `max` reasoning effort.**

> **All reviews, audits, verification, validation, critique, cross-checking, and final acceptance must use exclusively `gpt-6.1-sol` with `medium` reasoning effort.**

> **No Astra. No automatic routing. No fallback. No model substitution. Fail closed whenever the required configuration cannot be guaranteed.**

Apply this together with the subagent-first execution directive:

**delegate aggressively, parallelize aggressively, implement with GPT-6-Luna max, independently review with GPT-6.1-Sol medium, route findings back to GPT-6-Luna for fixes, re-review with GPT-6.1-Sol, and repeat until the complete goal is independently verified.**
