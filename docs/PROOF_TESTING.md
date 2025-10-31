# Proof and Property Testing Strategy

## Scope

- Validate core invariants used by pruning and heuristics:
  - Admissibility: $0 \le h(s) \le h^*(s)$.
  - Consistency: $h(s) \le c(s,s') + h(s')$.
  - Safe pruning: $g + ub > \text{best\_ub} \implies$ skip enqueue.
  - Upper bound monotonicity: $\text{best\_ub}$ never increases.

## Layers

1. Property-based testing (proptest)
   - Randomly generate valid small instances/states.
   - Properties:
     - `lower_bound(s) ≤ upper_bound(s)` and both nonnegative for all target
       types.
     - Consistency over all single-step successors.
     - Tiny exact checks: for very small instances (n≤3), compute optimal
       `h*(s)` via bounded search; assert `h(s) ≤ h*(s)`, `g+ub ≥ h*(s)`.
   - Run in CI with limited cases; nightly with higher case counts.

2. Differential testing vs no-prune
   - For randomized small instances: ensure prune-on/off return same optimal
     cost and that prune expands ≤ no-prune.
   - Record percentiles to detect regressions.

3. Contracts in Rust (contracts crate)
   - Use `contracts` (or `contracts-rs`) to add runtime design-by-contract
     assertions for pure helpers:
     - `infra_mult` range and monotonicity.
     - Denominator calculations (`civUpper`) non-decreasing in added civilians.
     - Non-negativity of all costs, bounds.
   - Contracts fire in debug/test builds to catch violations early; can be
     compiled out in release.

4. Bounded model checking (Kani)
   - Symbolically explore all paths up to small bounds:
     - Encode 2–3 node instances with small slot caps.
     - Assert admissibility/consistency/pruning decisions across all symbolic
       inputs within bounds.
   - Scope to arithmetic and decision functions to keep proof search tractable.

5. Deductive verification (Creusot / Prusti / Verus)
   - Extract spec-pure helpers and annotate pre/post conditions:
     - Non-negativity, monotonicity, and dominance relations for
       lower/upper-bound components.
   - Prefer Creusot/Prusti for Rust code; evaluate Verus for deeper proofs if
     acceptable to constrain subset.

## Workflow

- CI matrix:
  - cargo test (unit + proptest minimal cases).
  - contracts assertions enabled in test builds.
  - optional: kani proofs for tiny harnesses; creusot/prusti verification for
    spec modules (allow-failure at first).
  - nightly job: proptest with increased cases/timeouts.

## Roadmap

- Phase 1: Expand proptest and contracts assertions; add tiny exact solvers.
- Phase 2: Introduce Kani harnesses for prune decisions and bound arithmetic.
- Phase 3: Add Creusot/Prusti contracts for spec helpers; gradually prove key
  lemmas.
- Phase 4: Consider Verus for end-to-end properties on a reduced core if needed.
