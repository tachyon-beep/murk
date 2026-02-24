# Bug Report

**Date:** 2026-02-20
**Reporter:** crate-audit-wave4
**Severity:** [ ] Critical | [ ] High | [x] Medium | [ ] Low

## Affected Crate(s)

- [x] murk-obs

## Engine Mode

- [x] Both / Unknown

## Summary

Two related per-observation allocation issues:

1. **Per-agent scratch (plan.rs:1009-1010):** `execute_agent_entry_pooled` allocates two `Vec` (`scratch` and `scratch_mask`) on every invocation -- once per agent-entry per agent. For 1000 agents with a 7x7 region, this is 2000+ heap allocations per observation step.

2. **Fixed entries re-gathered per agent (plan.rs:822-846):** The loop re-executes the gather for all `fixed_entries` for every agent, despite fixed entries producing identical output. For N agents with M fixed elements, this is N*M redundant gathers.

Both are hot-path issues in RL training loops with many agents.

## Expected Behavior

Pre-allocate scratch buffers and reuse across agents (sequential processing allows this). Gather fixed entries once and memcpy per agent.

## Actual Behavior

Per-agent allocation and redundant computation.

## Additional Context

**Source:** murk-obs audit, Findings 5-6
**Files:** `crates/murk-obs/src/plan.rs:822-846`, `crates/murk-obs/src/plan.rs:1009-1010`
