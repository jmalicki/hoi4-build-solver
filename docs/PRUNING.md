# Pruning Strategy

## Overview

- We use A\* with prune-before-enqueue: for each generated successor `s'`, we compute `f(s') = g(s) + c(s,s') + h(s')`
  and consult anytime upper bounds before touching the frontier.
- If `f(s') ≥ best_ub`, we prove `s'` cannot improve the current best solution and we do not enqueue it.
- Additionally, we may consult component-level upper bounds (e.g., remaining capacity per node set) to rule out whole
  branches early.

## Invariants

- Admissibility: `h(s) ≤ h*(s)` for all `s` (never overestimates true cost-to-go), per target type.
- Consistency: `h(s) ≤ c(s,s') + h(s')` for every transition; ensures monotone `f` along optimal paths and avoids
  re-expansions.
- Upper bound monotonicity: `best_ub` is initialized to `+∞`, decreases when a better complete solution is found, and
  never increases.

## Decision Flow (per successor)

1. Compute `step_cost = c(s,s')`, `g' = g(s) + step_cost`, and `h' = h(s')`.
2. Compute `f' = g' + h'`.
3. If `f' ≥ best_ub`, drop `s'` (do not enqueue).
4. If component-level UB says the subtree rooted at `s'` cannot beat `best_ub`, drop `s'`.
5. Otherwise, attempt `decrease_key` or `enqueue`.

## Anytime Upper Bound

- We maintain `best_ub` from a greedy plan (e.g., convert-then-build) recomputed from current `s` or the start.
- `best_ub` provides a safe cut: any `f' ≥ best_ub` cannot lead to a better solution.

## Target Types

- Military: goal is `Σ mil ≥ target`; heuristics/UBs account for conversions and infra multipliers.
- Civilian: goal is `Σ civ ≥ target`.
- Factories: goal is `Σ (civ + mil) ≥ target`.

## Debug Metrics

- `pruned_pre`: count of states skipped by prune-before-enqueue.
- `pruned_rebuild`: count during periodic heap cleanup (should be rare).
- Average `f` on heap, total states allocated, and iterations.

## Testing

- Unit tests assert consistency for sampled transitions per target type.
- Synthetic instances with known optimal cost `C*`: verify that any `f ≥ C*` state is skipped (never enqueued), and pops
  are strictly fewer than no-prune.

## Operational Notes

- Prune must occur before enqueue/decrease-key to avoid churn.
- All calculations use the active target type; mixing target assumptions breaks admissibility/consistency.

See `docs/PROOF_TESTING.md` for a full strategy covering property-based testing, contracts, model checking with Kani,
and deductive verification (Creusot/Prusti/Verus).
