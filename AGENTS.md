<!-- filigree:instructions:v3.0.0rc12:65e6fb25 -->
## Filigree Issue Tracker

`filigree` tracks tasks for this project. Data lives in `.filigree/`. Prefer
the MCP tools (`mcp__filigree__*`) when available; fall back to the `filigree`
CLI otherwise.

### Workflow

```bash
# At session start
filigree session-context                            # ready / in-progress / critical path

# Pick up the next startable issue (atomic claim + transition into its working status)
filigree start-next-work --assignee <name>
# ...or claim a specific issue
filigree start-work <id> --assignee <name>

# Do the work, commit, then
filigree close <id>
```

Use the atomic claim+transition verbs — `work_start` / `work_start_next`
(MCP) or `start-work` / `start-next-work` (CLI). Do **not** chain
`work_claim` (MCP) or `filigree claim` (CLI) with a subsequent status
update — the two-step form races against other agents; the combined verb is
atomic.

**Ready ≠ startable.** The working status is type-specific (tasks →
`in_progress`, features → `building`). Bugs start at `triage`, which has no
single-hop transition into work (`triage → confirmed → fixing`), so a triage
bug is *ready* but not directly *startable*: `work_start` on one returns
`INVALID_TRANSITION` naming the next status, and `work_start_next` skips it.
`work_ready` items carry a `startable` flag (plus a `next_action` hint when
false). Pass `advance=true` (MCP) / `--advance` (CLI) to walk the soft
transitions to the nearest working status automatically.

### Observations: when (and when not) to use them

`observation_create` is a fire-and-forget scratchpad for *incidental* defects — things
you notice *outside the scope of your current task* (a code smell in a
neighbouring file, a stale TODO, a missing test for an edge case you happened
to spot). Notes expire after 14 days unless promoted. Include `file_path` and
`line` when relevant. At session end, skim `observation_list` and either
`observation_dismiss` or `observation_promote` for what has accumulated.

**You fix bugs in your currently defined scope. You do NOT use observations
to finish work prematurely.** If a defect, gap, or follow-up belongs to your
current task, you own it — handle it as part of that task: fix it now, expand
the task's scope, file a proper issue with a dependency, or surface it to the
user. Filing it as an observation and closing the task is *not* completing
the task; it is shipping known-broken work and hiding the debt in a 14-day
expiring scratchpad. The test is "would I have noticed this even if I weren't
working on this task?" If no, it's task scope, not an observation.

### Priority scale

- P0: Critical (drop everything)
- P1: High (do next)
- P2: Medium (default)
- P3: Low
- P4: Backlog

### Reaching for tools

MCP tool schemas describe each tool; `filigree --help` and `filigree <verb>
--help` are the authoritative CLI reference. You do not need to memorise
either catalogue. The verbs you will reach for most:

- **Find work:** `work_ready`, `work_blocked`, `issue_list`, `issue_search`
- **Claim work:** `work_start`, `work_start_next`
- **Update:** `comment_add`, `label_add`, `issue_update`, `issue_close`
- **Admin (irreversible):** `issue_delete` (MCP) / `delete-issue` (CLI) —
  hard-deletes a terminal issue and its rows; `admin_undo_last` cannot reverse it.
- **Scratchpad:** `observation_create`, `observation_list`, `observation_promote`, `observation_dismiss`
- **Cross-product entity bindings (ADR-029):** `entity_association_add`,
  `entity_association_remove`, `entity_association_list`,
  `entity_association_list_by_entity`. Used when a sibling tool (e.g.
  Loomweave) needs to bind a Filigree issue to a function, class, or
  module identifier it owns. The `entity_id` is an opaque external string
  from Filigree's perspective and may be a `loomweave:eid:...` SEI or a legacy
  locator; callers may also supply `entity_kind` explicitly. The consumer (the sibling tool's read
  path) does drift detection against the stored
  `content_hash_at_attach`. `entity_association_list_by_entity` is the
  reverse-lookup surface — given an opaque external entity ID, return every
  Filigree issue bound to it (project isolation is by DB file). Also
  reachable over HTTP as
  `GET/POST /api/issue/{issue_id}/entity-associations`,
  `DELETE /api/issue/{issue_id}/entity-associations?entity_id=…`,
  and `GET /api/entity-associations?entity_id=…`.
- **Health:** `stats_get`, `metrics_get`, `mcp_status_get`

Pass `--actor <name>` (CLI) so events attribute to your agent identity. It
works in either position — before the verb (`filigree --actor X update …`) or
after it (`filigree update … --actor X`); the post-verb value overrides the
group-level one.

### Error handling

Errors return `{error: str, code: ErrorCode, details?: dict}`. Switch on
`code`, not on message text. Codes: `VALIDATION`, `NOT_FOUND`, `CONFLICT`,
`INVALID_TRANSITION`, `PERMISSION`, `NOT_INITIALIZED`, `IO`,
`INVALID_API_URL`, `FILE_REGISTRY_DISPLACED`, `REGISTRY_UNAVAILABLE`,
`LOOMWEAVE_REGISTRY_VERSION_MISMATCH`, `LOOMWEAVE_OUT_OF_SYNC`,
`BRIEFING_BLOCKED`, `STOP_FAILED`, `SCHEMA_MISMATCH`, `INTERNAL`.

On `INVALID_TRANSITION`, call `workflow_transition_list` (MCP) or
`filigree transitions <id>` to see what the workflow allows from here.

Two failure modes deserve a specific response:

- **`SCHEMA_MISMATCH`** — the installed `filigree` is older than the project
  database. The error message contains upgrade guidance. Surface it to the
  user; do not retry.
- **`ForeignDatabaseError`** — filigree found a parent project's database
  but no local `.filigree.conf`. Run `filigree init` in the current
  directory. Do **not** `cd` upward to a different project unless that was
  the actual intent.
<!-- /filigree:instructions -->

<!-- wardline:instructions:v1:bcd19330 -->
This project uses **wardline** as its trust-boundary gate. Before handing back code that touches external input, run `wardline scan . --fail-on ERROR` (exit 0 = clean, 1 = gate tripped, 2 = wardline error) and fix findings at the boundary, not the sink. The full scan -> explain -> fix -> rescan loop and the baseline-vs-waiver discipline live in the `wardline-gate` skill and in `docs/agents.md`.
<!-- /wardline:instructions -->

<!-- legis:instructions:v1.0.0:6604fe0c -->
## Legis (git/CI + governance)

Legis is the git/CI and governance layer of the Weft suite. Reach for it when a policy fires at the CI/git boundary and a change needs a *recordable* override or human sign-off, when you need governance attestations keyed to stable code identity (SEI), or when you need git/CI context — branches, commits, pull requests, check outcomes, and the Loomweave-bound rename feed — around the work. Enforcement is graded: agent-programmable policy cells decide whether a violation self-clears with an audit trail, is judged inline, or escalates to a human; every decision lands in an append-only, SEI-keyed audit trail that survives rename/move.

Prefer the `mcp__legis__*` MCP tools when available; fall back to the `legis` CLI.

CLI subcommands:

- `serve` — run the Legis API server.
- `mcp` — run the Legis MCP stdio server (launch-bound `--agent-id`).
- `check-override-rate` — exit 1 if the override-rate gate is FAIL (for CI).
- `governance-gate` — run governance CI gates (currently the override-rate gate).
- `sei-backfill` — resolve legacy locator-keyed governance records through Loomweave batch resolve.
- `policy-boundary-check` — fail when `@policy_boundary` metadata lacks current behavioural evidence.

Full command + MCP-tool reference: see the `legis-workflow` skill.
<!-- /legis:instructions -->

<!-- loomweave:instructions:v1.1.0-rc3:5a7788e7 -->
## Loomweave (code archaeology)

This repo is indexed by Loomweave: it has pre-extracted the tree into a
queryable map of entities (functions, classes, modules, files), the call /
reference / import edges between them, and subsystem clusters. Before grepping
or re-reading the tree to answer "what calls X", "where is X defined", "what
subsystem owns X", or "find the thing that does Y" — ask Loomweave's MCP tools
(`mcp__loomweave__*`): `entity_find`, `entity_at`, `entity_callers_list`,
`entity_neighborhood_get`, `project_status_get`.

`entity_find` is the grep replacement for "find the thing that does Y": it
matches a concept word by substring over name, summary, and docstring content
(e.g. `library` finds `LibraryService`), with no embeddings required — reach for
it before grepping. Semantic *ranking* is the separate, opt-in
`entity_semantic_search_list`.

Entity IDs are `{plugin}:{kind}:{qualified_name}` (e.g.
`python:function:pkg.mod.func`); subsystems are `core:subsystem:{hash}`. You
rarely type IDs — get one from `entity_find` or `entity_at`, then copy it
verbatim into the next tool.

Index freshness and counts: `project_status_get` (or the `loomweave://context`
resource). If the index is stale, run `loomweave analyze <path>`.

LLM summaries (`entity_summary_get`) are off by default and need a configured live
provider; `project_status_get` reports the posture and `loomweave config check`
explains how to enable it.

Full workflow: the `loomweave-workflow` skill.
<!-- /loomweave:instructions -->
