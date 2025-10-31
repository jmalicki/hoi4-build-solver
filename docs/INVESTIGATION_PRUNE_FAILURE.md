# Investigation Plan: Pruning Optimality Failure

## Problem Statement

The test `prune_does_not_expand_more_than_no_prune_and_cost_matches` is failing because:
- **With pruning**: finds cost `11200.0`
- **Without pruning**: finds cost `32347.619...`
- **Expected**: both should find the same optimal cost

This indicates either:
1. The upper bound heuristic is not admissible (not ≥ optimal cost)
2. The pruning logic has a bug in maintaining `best_ub`
3. The test assumption is incorrect (pruning may not preserve optimality in this domain)

## Investigation Strategy

Based on `PROOF_TESTING.md`, we'll use a layered approach from most practical to most rigorous.

---

## Phase 1: Property-Based Testing (Immediate Priority)

**Status**: Partially implemented - existing proptest checks consistency but not admissibility

### 1.1 Add Upper Bound Admissibility Property Tests

**What to test**: For small, tractable instances, verify that the upper bound is ≥ the actual optimal cost.

**Implementation**:
```rust
#[test]
fn prop_upper_bound_admissible() {
    // For very small instances (≤2 nodes, ≤3 slots, ≤2 remaining target),
    // compute exact optimal cost via exhaustive search
    // Assert: upper_bound(s) >= exact_optimal_cost(s)
}
```

**Challenges**:
- Exhaustive search only feasible for tiny instances
- Need to implement a simple exact solver or BFS

**Benefits**:
- Can catch admissibility violations immediately
- Runs in CI without additional tools
- Provides concrete counterexamples

### 1.2 Add Differential Testing for Pruning Correctness

**What to test**: For random small instances, ensure prune-on/off find same cost and prune expands ≤ nodes.

**Implementation**:
```rust
#[test]
fn prop_prune_preserves_optimality() {
    // Generate random small instances
    // Run with prune=true and prune=false
    // Assert: cost_prune == cost_no_prune (within epsilon)
    // Assert: nodes_prune <= nodes_no_prune
    // Record statistics for regression detection
}
```

**Benefits**:
- Catches pruning bugs on diverse inputs
- Already have this for one specific case, generalize it
- Can run many cases to find edge cases

### 1.3 Add Invariant Checks for `best_ub` Monotonicity

**What to test**: `best_ub` should only decrease (never increase) during search.

**Implementation**:
- Add debug-only tracking of `best_ub` history
- In test builds, assert `best_ub_{i+1} <= best_ub_i`

**Benefits**:
- Catches bugs in pruning logic itself
- Low overhead (can be debug-only)
- Simple to implement

---

## Phase 2: Runtime Contracts (Quick Win)

**Status**: Not implemented

### 2.1 Add Basic Invariant Checks

**What to check**:
- `upper_bound(s) >= 0` (non-negativity)
- `lower_bound(s) <= upper_bound(s)` (bound ordering) 
- `best_ub` only decreases (monotonicity)
- All costs are non-negative

**Implementation**: Simple `debug_assert!` statements in key locations:
- After `upper_bound()` calls
- After `best_ub` updates
- In solver main loop

**Benefits**:
- Zero-dependency solution
- Catches violations immediately with good error messages
- Can be compiled out in release builds

### 2.2 Consider `contracts-rs` for More Formal Contracts

**When to use**: If we want more sophisticated pre/post conditions

**Trade-offs**:
- Adds dependency
- More powerful but requires learning curve
- Overkill if `debug_assert!` is sufficient

**Recommendation**: Start with `debug_assert!`, consider `contracts-rs` if we need more.

---

## Phase 3: Exact Solver for Small Instances (Most Valuable)

**Status**: Not implemented

### 3.1 Implement Exhaustive Search Solver

**Purpose**: Compute exact optimal cost for very small instances to verify heuristics.

**Implementation**:
- Simple BFS or DFS with memoization
- Only for tiny instances (≤2 nodes, ≤3 slots, ≤2 targets)
- Compare against heuristic bounds

**What to verify**:
- `lower_bound(s) <= exact_optimal(s)` (admissibility of lower bound)
- `upper_bound(s) >= exact_optimal(s)` (admissibility of upper bound)
- `exact_optimal(s)` for actual optimal solutions

**Benefits**:
- Provides ground truth for heuristic validation
- Finds specific counterexamples
- Can be used in proptest to check bounds on random small instances

**Challenges**:
- Only feasible for very small state spaces
- Need careful bounds checking to avoid infinite loops

---

## Phase 4: Advanced Verification (Future Work)

### 4.1 Kani (Bounded Model Checking)

**When to use**: After we have strong suspicion about specific arithmetic or bound calculations

**Scope**:
- Verify `infra_mult()` calculations
- Verify `civUpper` denominator calculations
- Verify basic bound arithmetic

**Trade-offs**:
- Symbolic execution is powerful but can be slow
- Best for isolated pure functions
- Requires learning Kani's syntax

**Recommendation**: Defer until we've exhausted easier approaches.

### 4.2 Deductive Verification (Prusti/Creusot/Verus)

**When to use**: If we need formal proof of admissibility

**Trade-offs**:
- Most rigorous but requires significant effort
- Would need to extract spec-pure helper functions
- Overkill for initial investigation

**Recommendation**: Only if we find the issue requires deep theoretical work.

---

## Immediate Action Plan

### Step 1: Add Debug Assertions (1 hour)
1. Add `debug_assert!` for `upper_bound >= 0` and `lower_bound <= upper_bound`
2. Add tracking of `best_ub` changes to verify monotonicity
3. Run failing test with assertions enabled

**Expected outcome**: May catch the bug immediately or provide clearer error message.

### Step 2: Implement Exact Solver for Tiny Instances (2-3 hours)
1. Write simple BFS solver for instances with ≤2 nodes, ≤3 slots, ≤2 targets
2. Compare heuristic bounds against exact optimal
3. Add property test: `upper_bound(s) >= exact_optimal(s)` for small instances

**Expected outcome**: Will reveal if upper bound is inadmissible on specific states.

### Step 3: Generalize Differential Test (1 hour)
1. Convert `prune_does_not_expand_more_than_no_prune_and_cost_matches` to property test
2. Run on many small random instances
3. Record statistics (success rate, cost differences, node expansion ratios)

**Expected outcome**: Will find if the failure is systematic or specific to certain inputs.

### Step 4: Analyze Specific Failure Case (1-2 hours)
1. Run failing test with detailed logging
2. Trace `best_ub` values throughout search
3. Identify which states get pruned incorrectly
4. Compare upper bound values at pruning points

**Expected outcome**: Will pinpoint where pruning goes wrong.

---

## Hypothesis: What We Think Is Happening

Based on code analysis:

1. **Upper bound initialization** (line 39): `best_ub = upper_bound(start)` should be ≥ optimal cost
2. **Pruning at expanded state** (lines 91-97): If `cur_cost + ub_suffix > best_ub`, skip expanding entirely
3. **Pruning at successor** (lines 104-111): If `cost_value + ub_ns > best_ub`, prune the successor

**Potential issue**: Line 110 updates `best_ub = cost_value + ub_ns` when we don't prune. But `cost_value + ub_ns` is an upper bound on paths *through* that successor, not necessarily on the optimal solution. If we update `best_ub` to be too low, we may prune optimal paths later.

**Key question**: Is `best_ub = min(best_ub, cost_value + ub_ns)` always safe? This assumes that the upper bound on a successor is always ≥ the optimal cost through that successor, which requires the upper bound heuristic to be admissible.

**Test to verify**: Check if `upper_bound(successor) >= exact_optimal_cost_from_successor` for small instances.

---

## Success Criteria

1. ✅ **Identify root cause**: Understand why pruning finds different cost than no-prune
2. ✅ **Reproduce reliably**: Have property tests that catch the issue
3. ✅ **Fix or document**: Either fix the bug or document that the test assumption is incorrect
4. ✅ **Prevent regression**: Add tests/invariants to catch similar issues

---

## Open Questions

1. **Is the upper bound heuristic provably admissible?** 
   - Need to check if `upper_bound(state) >= optimal_cost_from(state)` always holds
   - The heuristic uses a greedy "convert then build" strategy - is this guaranteed to be ≥ optimal?

2. **Is the pruning logic correct?**
   - Does updating `best_ub = min(best_ub, cost_value + ub_ns)` preserve optimality?
   - What if `best_ub` decreases below the true optimal cost before we've explored optimal paths?

3. **Is the test assumption valid?**
   - In general, pruning in A* should preserve optimality if the heuristic is admissible
   - But upper-bound-based pruning is different from standard A* - does it still preserve optimality?

---

## Next Steps

1. Start with Step 1 (debug assertions) - quickest way to get more information
2. Implement exact solver (Step 2) - will provide ground truth
3. Analyze specific failure with detailed logging (Step 4) - will pinpoint the bug
4. Generalize test (Step 3) - will ensure we catch regressions

Prioritize in this order for maximum insight per unit time.

