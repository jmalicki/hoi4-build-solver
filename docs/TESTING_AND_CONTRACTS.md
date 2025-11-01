# Testing and Contracts Strategy

This document outlines our approach to using Rust's `contracts` crate for design-by-contract verification, with a focus
on performance-aware contract checking.

## Overview

We use the [`contracts`](https://github.com/ureq/contracts) crate (version 0.6.6) to add runtime design-by-contract
assertions to our Rust code. This provides:

- **Pre-conditions** (`#[requires]`): Validate inputs before function execution
- **Post-conditions** (`#[ensures]`): Validate outputs after function execution
- **Invariants**: Ensure data structure consistency across operations

## Performance Strategy: O(1) vs O(n) Checks

Our contract strategy is guided by performance considerations:

### O(1) Checks: Always Enabled

**Simple, constant-time validations remain active in production builds.**

These checks provide fast validation without measurable performance impact:

- **Bounds checking**: `idx < self.states.len()`
- **NaN/validity checks**: `!value.is_nan() && value >= 0.0`
- **Basic membership checks**: `self.in_open.contains(&idx)`
- **Simple property checks**: `self.states[idx].ref_count > 0`

**Example:**

```rust
#[requires(handle.index() < self.states.len(), "Handle index is valid")]
#[requires(!estimated_total_cost.is_nan() && estimated_total_cost >= 0.0, "Estimated total cost is valid")]
#[ensures(self.in_open.contains(&handle.index()), "State is in heap membership set")]
pub fn heap_push(&mut self, handle: &StateHandle<S, T>, estimated_total_cost: f64) {
    // ... implementation ...
}
```

### O(n) Checks: Test-Only

**Expensive checks that iterate over data structures are gated to test builds only.**

These checks are valuable for catching bugs during development but would significantly impact production performance:

- **Full invariant checks**: Iterating over all states to verify reference counting
- **Heap accounting verification**: Validating all indices in the heap
- **Cross-structure consistency**: Checking relationships across multiple collections

**Example:**

```rust
#[cfg(test)]
fn check_heap_accounting_invariants(&self) -> bool {
    let heap_len_ok = self.heap_len == self.in_open.len() && self.heap_len == self.open.len();
    let heap_bound_ok = self.in_open.iter().all(|&idx| idx < self.heap_bound);
    // ... more O(n) checks ...
}

#[cfg_attr(test, ensures(self.check_heap_accounting_invariants(), "Heap accounting invariants hold"))]
pub fn heap_push(&mut self, handle: &StateHandle<S, T>, estimated_total_cost: f64) {
    // ... implementation ...
}
```

## Implementation Pattern

### 1. O(n) Check Functions: Gated with `#[cfg(test)]`

Helper functions that perform expensive checks are only available in test builds:

```rust
/// Check that heap accounting invariants are satisfied.
///
/// This is O(n) where n is the heap size, so it's gated for test-only use.
#[cfg(test)]
fn check_heap_accounting_invariants(&self) -> bool {
    // ... O(n) validation logic ...
}
```

### 2. Contract Attributes: Conditional with `#[cfg_attr(test, ...)]`

Contract attributes that call O(n) checks use `#[cfg_attr(test, ...)]` to conditionally apply them:

```rust
// O(1) check: always active
#[ensures(self.in_open.contains(&handle.index()), "State is in heap membership set")]

// O(n) check: only in test builds
#[cfg_attr(test, ensures(self.check_heap_accounting_invariants(), "Heap accounting invariants hold"))]
pub fn heap_push(&mut self, handle: &StateHandle<S, T>, estimated_total_cost: f64) {
    // ... implementation ...
}
```

### 3. Dependency Configuration

The `contracts` crate is included as a **regular dependency** (not `dev-dependency`) so contracts are available in
production builds:

```toml
[dependencies]
contracts = "0.6.6"
```

This allows O(1) checks to run in production while O(n) checks are compiled out.

## Benefits

1. **Fast Feedback in Development**: O(n) checks catch invariant violations during testing
2. **Production Safety**: O(1) checks provide runtime validation without performance penalty
3. **Gradual Verification**: Start with simple checks, add O(n) checks as needed
4. **Clear Intent**: Code structure makes performance considerations explicit

## When to Use Each Type

### Use O(1) Checks For:

- Input validation (bounds, null checks, valid ranges)
- Output validation (expected properties, return value ranges)
- Critical invariants that must hold in production
- Fast membership/equality checks

### Use O(n) Checks For:

- Full data structure consistency verification
- Cross-reference validation (e.g., ensuring all states in heap are also in index map)
- Complex invariant checks that require iteration
- Debugging aids for development

## Examples from Codebase

### Example 1: State Pool Allocation

```rust
/// Returns the stable index to use for the new state.
#[ensures(ret < self.states.len(), "Returned index is valid")]
#[ensures(self.states[ret].ref_count == 0, "Allocated slot has zero ref count")]
#[ensures(self.states[ret].cost_from_start == f64::INFINITY, "Allocated slot has infinite cost")]
fn allocate_index(&mut self) -> usize {
    // O(1) post-conditions validate the allocated slot
    // O(n) free indices check would be gated with #[cfg_attr(test, ...)] if needed
}
```

### Example 2: Reference Count Decrement

```rust
#[requires(idx < self.states.len() || true, "Index valid or method returns early")]
#[ensures(idx >= self.states.len() || self.states[idx].ref_count == 0 || !self.free_indices.contains(&idx), "Freed state added to free_indices when ref_count reaches zero")]
#[cfg_attr(test, ensures(idx >= self.states.len() || self.check_ref_count_invariants(), "Ref count invariants hold after decrement"))]
pub fn decrement_ref_count(&mut self, idx: usize) {
    // O(1) checks validate immediate state
    // O(n) check validates full ref-count consistency across all states
}
```

## Related Documentation

- `docs/PROOF_TESTING.md`: Broader testing and verification strategy
- `docs/DESIGN.md`: Overall architecture and design decisions
- `src/hoi4_build_core/src/state_pool/pool.rs`: Implementation with contracts

## Future Considerations

- Consider using `#[cfg(debug_assertions)]` instead of `#[cfg(test)]` if we want O(n) checks in debug builds but not
  release builds
- Evaluate adding more O(n) checks as the codebase matures
- Consider static verification tools (Kani, Creusot) for deeper invariants
