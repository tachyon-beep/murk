# Performance Budget (Phase 3 Baseline)

## Purpose

This document defines the Phase 3 performance baseline and regression budgets for:

- `murk-obs` extraction throughput
- `murk-space` coordinate-to-rank lookup latency
- `murk-arena` publish/snapshot and sparse reuse throughput

These budgets gate Tasks 11-13 and provide the comparison point for Gate C.

## Baseline Capture

- Captured at: `2026-02-22 05:22:40 UTC`
- Host: `Linux 6.8.0-100-generic x86_64` (AMD Ryzen 9 7900X3D, 24 logical CPUs)
- Toolchain: `rustc 1.93.1`, `cargo 1.93.1`
- Command:

```bash
cargo bench -p murk-bench --bench obs_ops --bench space_ops --bench arena_ops -- --sample-size 20 --measurement-time 1
```

- Criterion note: numbers below use the center estimate from Criterion's `time: [low mid high]` output.

## Baseline Results

| Domain | Benchmark ID | Baseline time |
|---|---|---:|
| `murk-obs` | `obs_execute_fixed/all_10k` | `8.878 us` |
| `murk-obs` | `obs_execute_agents/agent_disk_r3/16` | `1.833 us` |
| `murk-obs` | `obs_execute_agents/agent_disk_r3/64` | `4.692 us` |
| `murk-obs` | `obs_execute_batch/fixed_all/16` | `103.54 us` *(added in Task 11)* |
| `murk-obs` | `obs_execute_batch/fixed_all/64` | `416.57 us` *(added in Task 11)* |
| `murk-space` | `space_rank_lookup/square4_10k` | `13.762 us` |
| `murk-space` | `space_rank_lookup/product_square4xline1d/4096` | `50.496 us` |
| `murk-arena` | `arena_publish_snapshot/borrowed_snapshot_10k` | `2.292 us` |
| `murk-arena` | `arena_owned_snapshot_10k` | `47.430 ms` |
| `murk-arena` | `arena_sparse_reuse/publish_sparse/128` | `1.616 us` |
| `murk-arena` | `arena_sparse_reuse/publish_sparse/1024` | `2.675 us` |

## Regression Budgets (No-Regression Guard)

Until Tasks 11-13 land, these benchmarks must stay within budget versus baseline.

| Benchmark ID | Max allowed time | Allowed regression |
|---|---:|---:|
| `obs_execute_fixed/all_10k` | `10.7 us` | `+20%` |
| `obs_execute_agents/agent_disk_r3/16` | `2.2 us` | `+20%` |
| `obs_execute_agents/agent_disk_r3/64` | `5.7 us` | `+22%` |
| `obs_execute_batch/fixed_all/16` | `124.3 us` | `+20%` |
| `obs_execute_batch/fixed_all/64` | `499.9 us` | `+20%` |
| `space_rank_lookup/square4_10k` | `17.2 us` | `+25%` |
| `space_rank_lookup/product_square4xline1d/4096` | `63.1 us` | `+25%` |
| `arena_publish_snapshot/borrowed_snapshot_10k` | `2.8 us` | `+22%` |
| `arena_owned_snapshot_10k` | `57.0 ms` | `+20%` |
| `arena_sparse_reuse/publish_sparse/128` | `2.0 us` | `+24%` |
| `arena_sparse_reuse/publish_sparse/1024` | `3.3 us` | `+23%` |

## Improvement Targets (Tasks 11-13)

These are the explicit "must improve" targets for the optimization tasks:

- Task 11 (`murk-obs`):
  - `obs_execute_agents/agent_disk_r3/64` improves by at least `20%` (`<= 3.75 us`).
- Task 12 (`murk-space`):
  - `space_rank_lookup/product_square4xline1d/4096` improves by at least `20%` (`<= 40.40 us`).
- Task 13 (`murk-arena`):
  - `arena_owned_snapshot_10k` improves by at least `15%` (`<= 40.32 ms`).
  - `arena_sparse_reuse/publish_sparse/1024` improves by at least `15%` (`<= 2.27 us`).

## Task 11 Result (`murk-obs`)

Captured with:

```bash
cargo bench -p murk-bench --bench obs_ops -- --sample-size 20 --measurement-time 1
```

| Benchmark ID | Baseline | Post-Task11 | Delta |
|---|---:|---:|---:|
| `obs_execute_fixed/all_10k` | `8.878 us` | `6.393 us` | `-27.99%` |
| `obs_execute_agents/agent_disk_r3/16` | `1.833 us` | `1.550 us` | `-15.45%` |
| `obs_execute_agents/agent_disk_r3/64` | `4.692 us` | `3.441 us` | `-26.66%` |

Task 11 target status:

- `obs_execute_agents/agent_disk_r3/64 <= 3.75 us`: **met** (`3.441 us`).

## Evaluation Rule

For each benchmark:

1. Run the same benchmark command as baseline.
2. Compare the current center estimate to baseline and budget.
3. Fail if any "no-regression" budget is exceeded.
4. For Tasks 11-13, also fail if the corresponding improvement target is not met.

## CI Integration

Nightly benchmark tracking already runs in `.github/workflows/bench.yml` and provides global alert/fail thresholds. This document is the authoritative per-scenario budget for Phase 3 work review.
