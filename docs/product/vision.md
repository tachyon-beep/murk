# Vision - Murk

## Purpose
Murk exists to let reinforcement-learning and real-time simulation teams build deterministic, high-throughput worlds without writing bespoke engine, observation, replay, and binding infrastructure for every experiment.

## Who it serves
- Primary: engineers and researchers building RL environments that need deterministic simulation, batched observation extraction, and reproducible replay.
- Secondary: game/simulation developers who need a small Rust engine with C/Python bindings and clear safety boundaries.
- Explicitly not: a general-purpose game engine, rendering stack, or unbounded workflow framework.

## Anti-goals
- Do not become a renderer, asset pipeline, or scene editor.
- Do not push entity semantics into Python post-processing when the Rust engine can own them deterministically.
- Do not accept silent correctness risks in identity, replay, or FFI safety to preserve convenience.

## Authority grant
Granted by: John Morrissey     Last reviewed: 2026-06-12
Review cadence: per release-planning session or on any vision change

Autonomous within strategy - the agent may, without asking:
  prioritize tracker issues, write PRDs/plans, update internal product docs,
  dispatch implementation work, run local checks, and accept work against
  recorded criteria.

Escalate before acting - the agent must get owner sign-off for:
  public release or announcement, publishing crates/PyPI artifacts, deleting
  data, changing licensing, changing this vision/strategy, or committing
  outward-facing compatibility/deprecation promises.
