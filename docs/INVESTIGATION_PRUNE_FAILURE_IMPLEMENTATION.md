# Implementation Plan: Investigating Pruning Optimality Failure

## Overview

This document provides a detailed, actionable implementation plan with phases, specific goals, and checkboxes for
investigating why `prune_does_not_expand_more_than_no_prune_and_cost_matches` is failing.

**Branch**: `investigate/proptest-failures` (or `investigate/prune-optimality-failure`)

---

## Phase 1: Add Debug Assertions and Runtime Invariant Checks

**Goal**: Add runtime checks to catch violations immediately and gather diagnostic information.

**Estimated Time**: 1-2 hours

### 1.1 Add Basic Invariant Checks in Heuristic Module

- [ ] Create test file: `src/hoi4_build_core/src/heuristic/invariants.rs` (optional module for debug checks)
- [ ] Add debug assertion in `lower_bound()`:

  ```rust
  #[cfg(debug_assertions)]
  debug_assert!(lb >= 0.0, "lower_bound must be non-negative");
  ```

- [ ] Add debug assertion in `upper_bound()`:

  ```rust
  #[cfg(debug_assertions)]
  debug_assert!(ub >= 0.0, "upper_bound must be non-negative");
  ```

- [ ] Add assertion that `lower_bound <= upper_bound` in test helper
- [ ] Run `cargo test --manifest-path src/hoi4_build_core/Cargo.toml --lib` to verify no regressions
- [ ] Run `cargo fmt --all --manifest-path src/hoi4_build_core/Cargo.toml`
- [ ] Run `pre-commit run --all-files` and fix any issues
- [ ] Test that assertions work: temporarily add `debug_assert!(false)` and verify it triggers

### 1.2 Add `best_ub` Monotonicity Tracking in Solver

- [ ] Add debug-only field `prev_best_ub: Option<f64>` to track `best_ub` history
- [ ] In `solve_and_reconstruct_core`, after each `best_ub` update (lines 94, 110), add:

  ```rust
  #[cfg(debug_assertions)]
  if let Some(prev) = prev_best_ub {
      debug_assert!(best_ub <= prev, "best_ub must never increase (was {}, now {})", prev, best_ub);
  }
  prev_best_ub = Some(best_ub);
  ```

- [ ] Run:

  ```bash
  cargo test --manifest-path src/hoi4_build_core/Cargo.toml --lib prune_does_not_expand_more_than_no_prune_and_cost_matches -- --nocapture
  ```

  and observe behavior

- [ ] Run `cargo fmt --all --manifest-path src/hoi4_build_core/Cargo.toml`
- [ ] Run `pre-commit run --all-files` and fix any issues

### 1.3 Add Test for Invariant Checking

- [ ] Create test in `src/hoi4_build_core/src/core/mod.rs`:

  ```rust
  #[test]
  fn test_heuristic_bounds_invariants() {
      // Test that bounds satisfy basic invariants
  }
  ```

- [ ] Verify `lower_bound >= 0` for various states
- [ ] Verify `upper_bound >= 0` for various states
- [ ] Verify `lower_bound <= upper_bound` for various states
- [ ] Run `cargo test --manifest-path src/hoi4_build_core/Cargo.toml --lib test_heuristic_bounds_invariants`
- [ ] Commit with message: `test: add basic heuristic bound invariant checks`

### 1.4 Commit Phase 1

- [ ] Ensure all code is formatted: `cargo fmt --all --manifest-path src/hoi4_build_core/Cargo.toml`
- [ ] Run `pre-commit run --all-files` and fix all issues
- [ ] Run full test suite: `cargo test --manifest-path src/hoi4_build_core/Cargo.toml --lib`
- [ ] Review `git diff` for unintended changes
- [ ] Commit: `feat: add debug assertions for heuristic and pruning invariants`

**Conventional Commit Format**:

```text
feat: add debug assertions for heuristic and pruning invariants

Add runtime invariant checks to detect violations early:
- Non-negativity checks for lower_bound and upper_bound
- Assertion that lower_bound <= upper_bound
- Monotonicity tracking for best_ub during search

These debug-only assertions help diagnose the pruning
optimality failure without impacting release builds.

Part of investigating why prune_does_not_expand_more_than_no_prune_and_cost_matches fails.
```

---

## Phase 2: Implement Exact Solver for Tiny Instances

**Goal**: Build a ground-truth solver to verify heuristic admissibility on small instances.

**Estimated Time**: 3-4 hours

### 2.1 Create Exact Solver Module

- [ ] Create file: `src/hoi4_build_core/src/core/exact_solver.rs`
- [ ] Define function signature:

  ```rust
  pub fn exact_optimal_cost(
      desc: &[NodeDesc],
      start: &State,
      target_type: TargetType,
      target: i32,
  ) -> Option<f64>
  ```

- [ ] Implement BFS with state memoization
- [ ] Add bounds checking: only solve if `desc.len() <= 2`, `max(slots) <= 3`, `target <= 2`, total states < 10,000
- [ ] Return `None` if instance is too large
- [ ] Add unit tests for trivial cases (already at target, impossible cases)
- [ ] Run `cargo test --manifest-path src/hoi4_build_core/Cargo.toml --lib exact_solver` (or module name)
- [ ] Run `cargo fmt --all --manifest-path src/hoi4_build_core/Cargo.toml`
- [ ] Run `pre-commit run --all-files` and fix any issues

### 2.2 Test Exact Solver Correctness

- [ ] Write test cases:
  - [ ] Test: start state already satisfies target (should return 0.0)
  - [ ] Test: impossible target (should return None or f64::INFINITY)
  - [ ] Test: simple 1-node, 1-slot case with known optimal solution
  - [ ] Test: compare against manual calculation for tiny instance
- [ ] Verify solver finds known optimal solutions
- [ ] Run `cargo test --manifest-path src/hoi4_build_core/Cargo.toml --lib` with exact solver tests
- [ ] Run `cargo fmt --all --manifest-path src/hoi4_build_core/Cargo.toml`
- [ ] Run `pre-commit run --all-files` and fix any issues

### 2.3 Add Property Test for Upper Bound Admissibility

- [ ] Create property test in `src/hoi4_build_core/src/core/mod.rs`:

  ```rust
  #[test]
  fn prop_upper_bound_admissible_on_small_instances() {
      // Generate small instances (≤2 nodes, ≤3 slots, ≤2 target)
      // For each, compute exact_optimal_cost and upper_bound
      // Assert: upper_bound >= exact_optimal_cost
  }
  ```

- [ ] Use proptest to generate valid small instances
- [ ] Filter to instances where exact solver can run (check bounds)
- [ ] Assert admissibility: `prop_assert!(ub >= exact_opt || exact_opt.is_infinite())`
- [ ] Run `cargo test --manifest-path src/hoi4_build_core/Cargo.toml --lib prop_upper_bound_admissible` with many cases
- [ ] Run `cargo fmt --all --manifest-path src/hoi4_build_core/Cargo.toml`
- [ ] Run `pre-commit run --all-files` and fix any issues
- [ ] If violations found: document counterexamples and move to Phase 4

### 2.4 Add Property Test for Lower Bound Admissibility

- [ ] Create property test:

  ```rust
  #[test]
  fn prop_lower_bound_admissible_on_small_instances() {
      // Similar to upper_bound test but for lower_bound
      // Assert: lower_bound <= exact_optimal_cost
  }
  ```

- [ ] Run tests and verify
- [ ] Run `cargo fmt --all --manifest-path src/hoi4_build_core/Cargo.toml`
- [ ] Run `pre-commit run --all-files` and fix any issues

### 2.5 Commit Phase 2

- [ ] Ensure all code is formatted: `cargo fmt --all --manifest-path src/hoi4_build_core/Cargo.toml`
- [ ] Run `pre-commit run --all-files` and fix all issues
- [ ] Run full test suite: `cargo test --manifest-path src/hoi4_build_core/Cargo.toml --lib`
- [ ] Review `git diff` for unintended changes
- [ ] Commit: `feat: add exact solver for tiny instances and admissibility tests`

**Conventional Commit Format**:

```text
feat: add exact solver for tiny instances and admissibility tests

Implement exhaustive BFS solver for very small instances (≤2 nodes,
≤3 slots, ≤2 target) to provide ground truth for heuristic validation.

- Add exact_optimal_cost() function with bounds checking
- Add property tests to verify upper_bound >= exact_optimal (admissibility)
- Add property tests to verify lower_bound <= exact_optimal (admissibility)

This enables automated detection of inadmissible heuristics on small
instances, which can reveal systematic issues.

Part of investigating why prune_does_not_expand_more_than_no_prune_and_cost_matches fails.
```

---

## Phase 3: Generalize Differential Test and Add Detailed Logging

**Goal**: Find systematic issues and pinpoint where pruning goes wrong.

**Estimated Time**: 2-3 hours

### 3.1 Convert Specific Test to Property Test

- [ ] Extract test setup code from `prune_does_not_expand_more_than_no_prune_and_cost_matches`
- [ ] Create property test function:

  ```rust
  #[test]
  fn prop_prune_preserves_optimality() {
      // Generate small random instances
      // Run with prune=true and prune=false
      // Assert costs match and prune expands <= nodes
  }
  ```

- [ ] Use proptest to generate valid small instances
- [ ] Filter instances where solver can run in reasonable time
- [ ] Add tolerance for floating-point comparison (use 1e-9)
- [ ] Record statistics: success rate, cost differences, expansion ratios
- [ ] Run `cargo test --manifest-path src/hoi4_build_core/Cargo.toml --lib prop_prune_preserves_optimality` with many
      cases
- [ ] Run `cargo fmt --all --manifest-path src/hoi4_build_core/Cargo.toml`
- [ ] Run `pre-commit run --all-files` and fix any issues

### 3.2 Add Detailed Logging to Solver

- [ ] Add optional `verbose` flag to `SolveOptions`
- [ ] Log `best_ub` value after each update
- [ ] Log when states are pruned (with `cost_value + ub_ns` and `best_ub` values)
- [ ] Log when states are expanded vs skipped
- [ ] Create test that runs failing case with verbose logging enabled
- [ ] Run failing test with `RUST_LOG=debug` and capture output
- [ ] Analyze log output to identify problematic pruning decisions
- [ ] Run `cargo fmt --all --manifest-path src/hoi4_build_core/Cargo.toml`
- [ ] Run `pre-commit run --all-files` and fix any issues

### 3.3 Create Diagnostic Test for Failing Case

- [ ] Create dedicated test file: `src/hoi4_build_core/src/core/tests/prune_diagnostic.rs`
- [ ] Copy the failing test case exactly
- [ ] Add detailed logging at each pruning decision
- [ ] Output `best_ub` history, pruned states, and final costs
- [ ] Run test and save output to file:

  ```bash
  cargo test --manifest-path src/hoi4_build_core/Cargo.toml --lib prune_diagnostic -- --nocapture > prune_diagnostic_output.txt 2>&1
  ```

- [ ] Analyze output to identify root cause
- [ ] Document findings in test file or separate markdown file

### 3.4 Commit Phase 3

- [ ] Ensure all code is formatted: `cargo fmt --all --manifest-path src/hoi4_build_core/Cargo.toml`
- [ ] Run `pre-commit run --all-files` and fix all issues
- [ ] Run full test suite: `cargo test --manifest-path src/hoi4_build_core/Cargo.toml --lib`
- [ ] Review `git diff` for unintended changes
- [ ] Commit: `feat: add property test for pruning optimality and diagnostic logging`

**Conventional Commit Format**:

```text
feat: add property test for pruning optimality and diagnostic logging

Generalize the pruning optimality test to run on many random instances
and add detailed logging to diagnose failures.

- Convert prune_does_not_expand_more_than_no_prune_and_cost_matches
  to property test prop_prune_preserves_optimality
- Add verbose logging for best_ub updates and pruning decisions
- Create dedicated diagnostic test for the failing case

This helps identify if the failure is systematic or specific to
certain inputs, and provides detailed trace information.

Part of investigating why prune_does_not_expand_more_than_no_prune_and_cost_matches fails.
```

---

## Phase 4: Analyze Findings and Document Root Cause

**Goal**: Understand why pruning fails and document the issue.

**Estimated Time**: 2-3 hours

### 4.1 Analyze Exact Solver Results

- [ ] Review output from `prop_upper_bound_admissible` test
- [ ] If violations found:
  - [ ] Extract counterexample states
  - [ ] Manually verify that `upper_bound(state) < exact_optimal(state)`
  - [ ] Identify which component of the heuristic is causing the issue
  - [ ] Document in `docs/PRUNING_BUG_ANALYSIS.md`

### 4.2 Analyze Pruning Decision Logs

- [ ] Review diagnostic test output
- [ ] Identify first pruning decision that excludes optimal path
- [ ] Trace back to see why `best_ub` was set too low
- [ ] Check if `best_ub` monotonicity was violated
- [ ] Document sequence of events leading to failure

### 4.3 Compare Upper Bound vs Exact Optimal on Failing Case

- [ ] Run exact solver on the specific failing test case (if small enough)
- [ ] Compare `upper_bound(start)` vs `exact_optimal(start)`
- [ ] Check if upper bound is inadmissible on this specific state
- [ ] If not, trace through intermediate states in optimal path
- [ ] Verify upper bounds at each step

### 4.4 Create Analysis Document

- [ ] Create `docs/PRUNING_BUG_ANALYSIS.md`
- [ ] Document:
  - [ ] Observed behavior (costs don't match)
  - [ ] Root cause (inadmissible upper bound OR pruning logic bug)
  - [ ] Counterexamples (if found)
  - [ ] Proposed fixes (if identified)
  - [ ] Impact assessment
- [ ] Include relevant code snippets and test outputs
- [ ] Run `pre-commit run --all-files` on markdown files

### 4.5 Commit Phase 4

- [ ] Ensure all code is formatted: `cargo fmt --all --manifest-path src/hoi4_build_core/Cargo.toml`
- [ ] Run `pre-commit run --all-files` and fix all issues
- [ ] Review `git diff` for unintended changes
- [ ] Commit: `docs: analyze and document pruning optimality failure root cause`

**Conventional Commit Format**:

```text
docs: analyze and document pruning optimality failure root cause

Document findings from exact solver and diagnostic logging analysis.
Includes counterexamples, root cause identification, and proposed fixes.

Part of investigating why prune_does_not_expand_more_than_no_prune_and_cost_matches fails.
```

---

## Phase 5: Implement Fix (if Root Cause Identified)

**Goal**: Fix the identified issue.

**Estimated Time**: Variable (depends on root cause)

### 5.1 Implement Fix

- [ ] Based on analysis, implement fix for identified root cause
- [ ] Options:
  - [ ] Fix upper bound heuristic to be admissible
  - [ ] Fix pruning logic to maintain correct `best_ub`
  - [ ] Adjust test assumptions if issue is with test, not code
- [ ] Add unit tests for the fix
- [ ] Run `cargo test --manifest-path src/hoi4_build_core/Cargo.toml --lib` to verify fix
- [ ] Run `cargo fmt --all --manifest-path src/hoi4_build_core/Cargo.toml`
- [ ] Run `pre-commit run --all-files` and fix any issues

### 5.2 Verify Fix with Property Tests

- [ ] Re-run `prop_upper_bound_admissible` (should pass now)
- [ ] Re-run `prop_prune_preserves_optimality` (should pass now)
- [ ] Re-run original failing test: `prune_does_not_expand_more_than_no_prune_and_cost_matches`
- [ ] Run full test suite to ensure no regressions
- [ ] Run `cargo fmt --all --manifest-path src/hoi4_build_core/Cargo.toml`
- [ ] Run `pre-commit run --all-files` and fix any issues

### 5.3 Commit Fix

- [ ] Ensure all code is formatted: `cargo fmt --all --manifest-path src/hoi4_build_core/Cargo.toml`
- [ ] Run `pre-commit run --all-files` and fix all issues
- [ ] Run full test suite: `cargo test --manifest-path src/hoi4_build_core/Cargo.toml --lib`
- [ ] Review `git diff` for unintended changes
- [ ] Commit with appropriate type: `fix: ...` or `refactor: ...` or `test: ...`

**Conventional Commit Format** (example):

```text
fix: ensure upper bound heuristic is admissible

[Detailed description of the fix, what was wrong, and how it's fixed]

Fixes the issue where pruning would find suboptimal solutions because
the upper bound heuristic was not always >= optimal cost. [Rest of
explanation]

Closes #[issue-number] (if applicable)
```

---

## Phase 6: Create Pull Request

**Goal**: Submit work for review with comprehensive PR description.

### 6.1 Prepare Branch

- [ ] Ensure all phases are complete and committed
- [ ] Fetch latest `main`: `git fetch origin main`
- [ ] Rebase branch onto `main`: `git rebase origin/main` (or merge if preferred)
- [ ] Resolve any conflicts
- [ ] Run final `cargo test --manifest-path src/hoi4_build_core/Cargo.toml --lib`
- [ ] Run final `cargo fmt --all --manifest-path src/hoi4_build_core/Cargo.toml`
- [ ] Run final `pre-commit run --all-files` and fix any issues

### 6.2 Push Branch

- [ ] Push branch to remote: `git push origin investigate/prune-optimality-failure` (or branch name)
- [ ] Verify branch appears on GitHub

### 6.3 Create Pull Request

- [ ] Use GitHub CLI: `gh pr create --title "..." --body "..." --base main`
- [ ] Or use GitHub web interface

**PR Title** (Conventional Commits format):

```text
fix: investigate and fix pruning optimality failure
```

OR (if no fix yet):

```text
investigate: add tooling to diagnose pruning optimality failure
```

**PR Description Template**:

```markdown
## Problem

The test `prune_does_not_expand_more_than_no_prune_and_cost_matches` is failing because pruning finds a different (and
potentially suboptimal) solution than no-prune mode.

## Investigation Approach

This PR implements investigation tooling from [PROOF_TESTING.md](docs/PROOF_TESTING.md) to diagnose the issue:

### Phase 1: Debug Assertions

- Added runtime invariant checks for heuristic bounds
- Added monotonicity tracking for `best_ub`

### Phase 2: Exact Solver

- Implemented exhaustive BFS solver for tiny instances (≤2 nodes, ≤3 slots, ≤2 target)
- Added property tests to verify upper bound admissibility: `upper_bound >= exact_optimal`

### Phase 3: Property Tests and Logging

- Generalized failing test to property test running on many random instances
- Added detailed logging for pruning decisions

### Phase 4: Analysis

- [Status: Complete/In Progress/Not Started]
- [If complete: link to analysis doc]

## Findings

[Document what was discovered]

## Solution

[If fix implemented: describe the fix] [If not yet fixed: describe next steps]

## Testing

- [ ] All existing tests pass
- [ ] New property tests pass
- [ ] Exact solver verified on known cases
- [ ] Diagnostic logging reveals root cause

## Related Issues

Fixes #[issue-number] (if applicable) Related to #[issue-number] (if applicable)
```

### 6.4 Verify PR CI

- [ ] Wait for CI to run
- [ ] Check that all CI jobs pass
- [ ] If failures:
  - [ ] Review CI logs
  - [ ] Fix issues locally
  - [ ] Push fixes
  - [ ] Re-run CI

---

## Phase 7: Follow-up (After PR Review)

### 7.1 Address Review Comments

- [ ] Respond to review comments
- [ ] Make requested changes
- [ ] Run `cargo test --manifest-path src/hoi4_build_core/Cargo.toml --lib` after changes
- [ ] Run `cargo fmt --all --manifest-path src/hoi4_build_core/Cargo.toml` after changes
- [ ] Run `pre-commit run --all-files` after changes
- [ ] Push updates
- [ ] Request re-review if needed

### 7.2 Merge PR

- [ ] Ensure CI passes
- [ ] Ensure approval received
- [ ] Merge PR (or request merge if you don't have permissions)
- [ ] Delete branch after merge (if not auto-deleted)

---

## Quick Reference: Commands

### Testing

```bash
# Run all tests
cargo test --manifest-path src/hoi4_build_core/Cargo.toml --lib

# Run specific test
cargo test --manifest-path src/hoi4_build_core/Cargo.toml --lib test_name

# Run with output
cargo test --manifest-path src/hoi4_build_core/Cargo.toml --lib test_name -- --nocapture

# Run property tests with more cases
PROPTEST_CASES=1000 cargo test --manifest-path src/hoi4_build_core/Cargo.toml --lib prop_test_name
```

### Formatting

```bash
# Format Rust code
cargo fmt --all --manifest-path src/hoi4_build_core/Cargo.toml

# Check formatting (for CI)
cargo fmt --all --manifest-path src/hoi4_build_core/Cargo.toml -- --check
```

### Pre-commit

```bash
# Run all hooks
pre-commit run --all-files

# Run specific hook
pre-commit run rustfmt-core --all-files
```

### Git Workflow

```bash
# Check status
git status

# See what will be committed
git diff --cached

# Commit
git commit -m "type: subject

Body explaining what and why."
```

### Conventional Commits Format

```text
<type>(<scope>): <subject>

<body>

<footer>
```

**Types**: `feat`, `fix`, `docs`, `test`, `refactor`, `chore`, etc. **Scope**: Optional, e.g., `heuristic`, `solver`,
`pruning` **Subject**: Short description (<50 chars) **Body**: Detailed explanation **Footer**: Breaking changes,
related issues

---

## Notes

- Always run `cargo fmt` before committing
- Always run `pre-commit run --all-files` before pushing
- Write descriptive commit messages following Conventional Commits
- Add tests before implementing fixes
- Document findings as you go
- Keep PR descriptions comprehensive but focused
