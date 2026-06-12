---
name: legis-workflow
description: >
  This skill should be used when the user asks to explain or evaluate a policy cell,
  submit a graded override, check the override-rate CI gate, run a governance gate,
  read git branch/commit context, read the git-rename feed for Loomweave, gate a
  Filigree closure on verified binding evidence, route Wardline scan findings through
  governance, read recorded pull-request or CI check outcomes, run the
  policy-boundary-check, or back-fill SEI-keyed governance records — or when working
  in a project that uses legis for git/CI governance and graded enforcement.
---

# Legis Workflow

Legis is the git/CI and **governance** side of the Weft suite. This skill is the
depth behind the lean `CLAUDE.md` block: the full CLI reference, the MCP tool
catalogue, the error/recovery table, and the worked patterns an agent actually
runs. Keep it faithful to the installed `legis` — when in doubt, `legis --help`
and `legis <command> --help` are authoritative.

## What legis is

Legis answers *what changed, in which branch/commit/PR/check context, and what
governance or attestation state exists for that change?* It is an SEI **consumer**
(Loomweave remains the identity authority) and the suite's single governed judge —
**Wardline analyses trust; Legis governs it, one judge not two**. It does not own
issue state (Filigree) or code identity (Loomweave); it adds branch/commit/PR/check
context and a graded enforcement layer on top.

Enforcement is a **2×2** of policy *cells*, each agent-set, each a distinct
override flow:

| | Judge OFF | Judge ON |
|---|---|---|
| **Simple** | **chill** — agent self-reports a recordable override; human reviews async (`ACCEPTED_SELF`) | **coached** — an LLM wall evaluates the override *before* it records; `ACCEPTED_BY_JUDGE` or `BLOCKED` (not self-clearable) |
| **Complex** | **structured** — block + escalate; a human operator must sign off before the gate clears (`ESCALATED_PENDING`) | **protected** — full machinery: HMAC-signed verdicts, decay sweep, override-rate gate, operator override |

The operating invariant is **agent-first: humans on the loop, not in the loop.**
Every cell produces an append-only audit trail keyed on SEI, so the record survives
rename/move. The recorded override is the safety mechanism — an attributable audit
event, never a silent pass.

## Reaching the tools

Prefer the MCP tools (`mcp__legis__*`) when a Legis MCP server is attached; fall
back to the `legis` CLI otherwise. Each surface maps thinly over the same service
layer, so they agree on outcomes.

**Identity is launch-bound.** The MCP server is started with
`legis mcp --agent-id <name>`; that `--agent-id` is the actor for every override,
sign-off, and audit record the session produces. **No tool schema accepts an actor
argument** — you cannot spoof or override identity from a call. (Contrast the CLI's
`sei-backfill --actor`, which stamps appended backfill events from a one-shot
command, not an interactive session.)

The MCP transport is stdio JSON-RPC (one object per line). Tool errors come back as
`isError` results with a `structuredContent` envelope carrying `error_code`,
`message`, `recoverable`, and `next_action` (see Error handling).

## CLI reference

`legis <command> [flags]`. Most stores fall back to environment variables; flags
override.

### `legis serve` — run the Legis API server
- `--host` (default `127.0.0.1`), `--port` (default `8000`) — bind address.
- `--governance-db` — governance store URL (env `LEGIS_GOVERNANCE_DB`).
- `--check-db` — check store URL (env `LEGIS_CHECK_DB`).
- `--protected-policies` — comma-separated protected policy list (env `LEGIS_PROTECTED_POLICIES`).
- `--loomweave-url` — Loomweave identity API URL (env `LOOMWEAVE_API_URL`).
- `--filigree-url` — Filigree issue-tracker API URL (env `FILIGREE_API_URL`).
- `--binding-db` — sign-off binding ledger URL (env `LEGIS_BINDING_DB`).
- Judge flags (shared): `--judge-provider` (`openrouter`; omit to keep protected cells fail-closed), `--judge-model` (env `LEGIS_JUDGE_MODEL`), `--judge-max-tokens` (env `LEGIS_JUDGE_MAX_TOKENS`).

### `legis mcp` — run the MCP stdio server
- `--agent-id` (**required**) — launch-bound agent identity; the actor for all records this session.
- `--governance-db` (env `LEGIS_GOVERNANCE_DB`), `--check-db` (env `LEGIS_CHECK_DB`).
- `--policy-cells` — policy cell registry TOML path (env `LEGIS_POLICY_CELLS`).
- `--protected-policies` (env `LEGIS_PROTECTED_POLICIES`), `--loomweave-url` (env `LOOMWEAVE_API_URL`).
- Judge flags (shared): `--judge-provider`, `--judge-model`, `--judge-max-tokens`.

### `legis check-override-rate` — CI gate
Fails (exit 1) if the override-rate gate is `FAIL`. For CI use.
- `--db` — governance store URL (default mirrors the server's `LEGIS_GOVERNANCE_DB` / `DEFAULT_GOVERNANCE_DB`).

Prints `override-rate gate: <STATUS> (rate=…, sample=…)`. A missing SQLite DB under
`CI=true` (without `LEGIS_ALLOW_MISSING_GOVERNANCE_DB=1`) fails; otherwise it prints
`PASS_WITH_NOTICE` and exits 0. A failed hash-chain integrity check exits 1.

### `legis governance-gate` — run governance CI gates
Currently runs the override-rate gate (same implementation and `--db` semantics as
`check-override-rate`). Use this name for the general CI gate entry point.

### `legis sei-backfill` — resolve legacy locator-keyed records
Resolves legacy locator-keyed governance records through Loomweave batch resolve and
emits a JSON report.
- `--db` — governance store URL (env `LEGIS_GOVERNANCE_DB`).
- `--loomweave-url` (**required**) — Loomweave identity API URL.
- `--execute` — append backfill events (omit for a dry-run report).
- `--actor` (default `legis-sei-backfill`) — actor stamped on appended events.

### `legis policy-boundary-check` — boundary-evidence gate
Fails (exit 1) when `@policy_boundary` metadata lacks current behavioural evidence.
- `--root` (default `src`) — Python source root to scan.
- `--repo-root` (default `.`) — repo root for `test_ref` resolution.
- `--format` (`text` | `json`, default `text`) — human-readable lines vs machine-readable findings.

Prints `policy-boundary-check: PASS` (exit 0) when clean; otherwise one
`path:line: rule_id: qualname: reason` per finding (exit 1).

## MCP tool catalogue

All tools return a `structuredContent` JSON payload. Names are exact.

### Governance / policy
| Tool | Purpose |
|---|---|
| `policy_explain` | Explain which governance cell controls a policy/entity pair, whether that cell is enabled here, and which move the agent may make next. Reports `matched_rule` — the routing pattern that matched, or `null` when the policy fell through to `default_cell` (distinguishes a configured-but-disabled policy from an unconfigured name). |
| `policy_list` | List the policy-to-cell routing table (`default_cell` + the configured pattern `rules`) and every governance cell's **real** enabled state on this server. The complex tier (structured/protected) reports `enabled: false` without `LEGIS_HMAC_KEY`. No arguments. |
| `policy_evaluate` | Evaluate a policy against a target **without recording an override**. Returns outcome, detail, and any `provenance_gap`. |
| `override_submit` | Submit an override as the launch-bound agent. Routes to the governing cell and returns a discriminated outcome envelope (`ACCEPTED_SELF` / `ACCEPTED_BY_JUDGE` / `BLOCKED` / `ESCALATED_PENDING` / `NEED_INPUTS`). |
| `signoff_status_get` | Poll whether a **structured** sign-off request (by `seq`) has been cleared. |
| `override_rate_get` | Read the fixed operator force-past override-rate gate (status / rate / sample_size). Measures operator force-pasts; **not** movable by agent retries. |
| `scan_route` | Route Wardline scan findings through one cell, a `severity_map`, or a cell + `fail_on` threshold. Returns `ROUTED` on success; dirty unsigned artifacts return `WARDLINE_DIRTY_TREE` (`isError: true`) unless the dev dirty opt-in is enabled. |

### Git
| Tool | Purpose |
|---|---|
| `git_branch_list` | List local git branches and upstream divergence facts. |
| `git_commit_get` | Read one git commit by SHA or safe ref. |
| `git_rename_list` | List git rename evidence for a revision range (`rev_range`). |
| `git_rename_feed_get` | Loomweave-ready rename feed: committed renames over `base..head` plus optional uncommitted working-tree renames (`include_worktree`). |

### Pulls / checks
| Tool | Purpose |
|---|---|
| `pull_request_get` | Read recorded pull-request metadata (`number`) with joined check outcomes. |
| `check_list` | Read recorded CI/check outcomes for a `target_type` of `commit`, `branch`, or `pr` plus a `target`. |

### Filigree binding
| Tool | Purpose |
|---|---|
| `filigree_closure_gate_get` | Read whether legis holds **verified binding evidence** for closing a Filigree issue (`issue_id`). Requires the binding ledger to be enabled. |

### Override-submit outcomes (by cell)
- **chill** → `ACCEPTED_SELF` — self-cleared; human reviews asynchronously.
- **coached** / **protected** → `ACCEPTED_BY_JUDGE` (may be re-judged later) or `BLOCKED`. A `BLOCKED` verdict carries a `blocked_reason_code` (`RATIONALE_INSUFFICIENT` / `CODE_VIOLATION` / `POLICY_HARD_BLOCK` / `UNCLASSIFIED`), `self_clearable: false`, and `next_actions: [REVISE_CODE, REVISE_RATIONALE]`. A blocked attempt **does not count toward your override-rate** — you cannot self-clear past the judge.
- **structured** → `ESCALATED_PENDING` — human sign-off required; poll `signoff_status_get` with the returned `seq`.
- **protected** with missing inputs → `NEED_INPUTS` — supply the listed fields (e.g. `file_fingerprint`, `ast_path`) and resubmit.

Pass an `idempotency_key` on `override_submit` to make retries safe: a repeat with
the same request returns the original outcome; a reused key with a *different*
request is rejected (`INVALID_ARGUMENT`).

## Error handling

Tool errors carry `error_code`, `message`, `recoverable`, and a `next_action` hint.
Branch on `error_code`, not message text.

| `error_code` | Recoverable | `next_action` |
|---|---|---|
| `INVALID_ARGUMENT` | yes | Correct the tool arguments and retry. |
| `INVALID_CELL_SPEC` | yes | scan_route routing is server-owned and unconfigured by default. The operator sets `LEGIS_WARDLINE_CELL` (e.g. `=surface_only`) or `LEGIS_WARDLINE_CELL_BY_SEVERITY` out-of-band, then relaunches. (Request-side routing requires the `LEGIS_UNSAFE_WARDLINE_REQUEST_ROUTING` opt-in — discouraged.) The error message names which kind of cell spec was rejected. |
| `CELL_NOT_ENABLED` | yes | Two enablement tiers, by cell — both operator-enabled, out-of-band. Simple tier (chill/coached) is reachable WITHOUT a key: the operator maps the policy to a cell via `policy/cells.toml` or `LEGIS_POLICY_CELLS` (`LEGIS_DEV_DEFAULT_CELLS=1` selects the chill dev default), then relaunches. Complex tier (structured/protected and the binding ledger) additionally needs `LEGIS_HMAC_KEY` set by the operator out-of-band, then a relaunch. The error message names which cell is unenabled. |
| `NO_SUCH_REQUEST` | yes | Poll a known sign-off sequence returned by `override_submit`. |
| `NOT_FOUND` | yes | Refresh the target identifier and retry. |
| `UNKNOWN_TOOL` | yes | Call `tools/list` and use one of the advertised tool names. |
| `GIT_ERROR` | yes | Check the git ref or revision range and retry. |
| `SERVICE_ERROR` | yes | Inspect the error message before retrying. |
| `AUDIT_INTEGRITY_FAILURE` | **no** | Stop and ask an operator to inspect the governance trail. |
| `INTERNAL_ERROR` | **no** | Inspect the error message before retrying. |

`AUDIT_INTEGRITY_FAILURE` (raised on a failed hash-chain verification or a binding
ledger error) and `INTERNAL_ERROR` are **not recoverable** — do not retry; surface
them to a human. Everything else is recoverable by fixing the input or asking the
operator to enable a cell.

Two routing-specific notes for `scan_route`:
- Wardline routing is **server-owned**. Passing `cell` / `severity_map` / `fail_on`
  when the server already configures routing (`LEGIS_WARDLINE_CELL` /
  `LEGIS_WARDLINE_CELL_BY_SEVERITY`) returns `INVALID_CELL_SPEC`. Request-side
  routing is only honoured under the explicit `LEGIS_UNSAFE_WARDLINE_REQUEST_ROUTING=1`
  escape hatch.
- An unsigned dirty-tree dev artifact arriving where signed provenance is required
  is a typed recoverable failure, not a success: MCP returns
  `WARDLINE_DIRTY_TREE` with `isError: true` and nothing is governed. Commit for
  a signed artifact, or set `LEGIS_WARDLINE_ALLOW_DIRTY=1` to govern it unsigned
  in dev.

## Workflow patterns

### Evaluate a policy cell, then submit a graded override
```
policy_explain {policy, entity}        # which cell governs, is it enabled, what move is next
# read explanation.cell and available_moves (already filtered to agent-callable tools)
override_submit {policy, entity, rationale [, file_fingerprint, ast_path, idempotency_key]}
```
- **chill** → `ACCEPTED_SELF`; you are done, the human reviews the trail async.
- **coached/protected** → if `BLOCKED`, do not retry verbatim — `REVISE_CODE` or
  `REVISE_RATIONALE` per `next_actions`; the judge cannot be talked past and the
  blocked attempt costs you nothing on the override-rate.
- **structured** → `ESCALATED_PENDING`; poll `signoff_status_get {seq}` until
  `cleared: true`. Do not proceed on the gated change until then.
- **protected** → if `NEED_INPUTS`, supply `file_fingerprint` + `ast_path` (the
  bytes and AST node the judge binds its verdict to) and resubmit.

### Check the override-rate gate in CI
The gate measures **operator force-pasts**, not agent retries — a high rate means
the policy is miscalibrated or an operator is breaking their own rules.
```
# in-session read:
override_rate_get {}                    # → {status, rate, sample_size}
# CI step (exit 1 on FAIL):
legis check-override-rate --db <governance-db>
#   or the general entry point:
legis governance-gate --db <governance-db>
```

### Read the git-rename feed for Loomweave
Legis is the (contract-locked) rename provider Loomweave's SEI re-binding matcher
consumes.
```
git_rename_feed_get {base, head?, include_worktree?}
#   committed renames over base..head, plus optional uncommitted working-tree renames
# lower-level evidence over an explicit range:
git_rename_list {rev_range}
```

### Gate a Filigree closure on verified binding evidence
Before closing a governed Filigree issue, confirm Legis holds verified, SEI-keyed
sign-off binding evidence for it.
```
filigree_closure_gate_get {issue_id}    # requires the binding ledger to be enabled
# only close in Filigree once this reports verified binding evidence;
# Filigree retains lifecycle authority — Legis only certifies the evidence.
```
If the ledger is not enabled you get `CELL_NOT_ENABLED` — ask the operator to wire
`LEGIS_BINDING_DB` / `--binding-db`.

### Route Wardline findings through governance
```
scan_route {scan}                       # routing is server-owned; pass only the scan
# → ROUTED (governed into the configured cell), or WARDLINE_DIRTY_TREE with
#   isError:true (commit, or set LEGIS_WARDLINE_ALLOW_DIRTY=1 in dev)
```

### Gate boundary evidence in CI
```
legis policy-boundary-check --root src --repo-root . --format json
#   exit 1 with findings when @policy_boundary metadata lacks current behavioural evidence
```
