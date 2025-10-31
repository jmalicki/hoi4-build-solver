# Pluggable Heuristics Design

## Overview

This document describes the refactoring to support pluggable heuristics in the
HOI4 MDP solver. The goal is to enable experimentation with different heuristic
strategies while maintaining a clean, extensible architecture.

## Current State

Currently, the heuristic logic is embedded directly in `lib.rs`:

- `heuristic()` function: computes an admissible lower bound on remaining cost
- `upper_bound_convert_then_mil()` function: computes an upper bound for pruning

These functions have implementation-specific names that don't reflect their role
as pluggable components.

## Target Architecture

### Module Structure

```text
src/
  lib.rs              # Main solver logic
  heuristic/
    mod.rs            # Heuristic trait and factory functions
    standard.rs       # Current "best-case infra + conversion-aware" heuristic
```

### Heuristic Trait

The `Heuristic` trait defines the interface all heuristics must implement:

```rust
pub trait Heuristic: Send + Sync {
    /// Admissible lower bound on remaining cost from state `st`.
    ///
    /// Must satisfy: h(s) <= actual optimal cost from s to goal
    /// Returns a non-negative value.
    fn lower_bound(
        &self,
        st: &State,
        nodes: &[NodeDesc],
        target_type: TargetType,
        target: i32,
    ) -> f64;

    /// Upper bound on remaining cost from state `st`.
    ///
    /// Used for pruning: if g(s) + ub(s) > best_known_solution_cost,
    /// we can prune state s. Returns f64::INFINITY if no bound is known.
    fn upper_bound(
        &self,
        st: &State,
        nodes: &[NodeDesc],
        target_type: TargetType,
        target: i32,
    ) -> f64;

    /// Human-readable name for this heuristic (for debugging/logging).
    fn name(&self) -> &'static str;
}
```

### Current Implementation: `StandardHeuristic`

The current heuristic logic becomes `StandardHeuristic`, implementing the trait:

- `lower_bound()`: current `heuristic()` function logic
- `upper_bound()`: current `upper_bound_convert_then_mil()` function logic
- `name()`: returns `"standard"`

### Solver API Changes

`lib.rs::solve_and_reconstruct()` will accept a heuristic parameter:

```rust
fn solve_and_reconstruct_internal(
    nodes: Vec<(String, i32, i32, i32, i32)>,
    target_type: TargetType,
    target: i32,
    heuristic: Box<dyn Heuristic>,
    verbose: bool,
    print_every: usize,
    prune: bool,
) -> Result<(Vec<(String, String)>, Vec<(i32, i32, i32)>, f64), String>
```

The public Python function `solve_and_reconstruct()` will:

1. Parse heuristic name (default: `"standard"`)
2. Create heuristic via `heuristic::create_by_name()`
3. Call internal function with the heuristic

### Python API

Add a Python-exposed function to create heuristics:

```rust
#[pyfunction]
fn create_heuristic(name: &str) -> PyResult<Box<dyn PyHeuristic>> {
    // Returns an opaque PyHeuristic object
}
```

`PyHeuristic` is a Python-wrapped version that implements `Heuristic` and can be
passed to the solver.

Alternatively, simpler design: heuristic name is just a string parameter to
`solve_and_reconstruct()`, and the factory is internal.

### Heuristic Factory

```rust
pub fn create_by_name(name: &str) -> Result<Box<dyn Heuristic>, String> {
    match name {
        "standard" => Ok(Box::new(StandardHeuristic)),
        // Future heuristics: "greedy", "relaxed", etc.
        _ => Err(format!("Unknown heuristic: {}", name)),
    }
}
```

## Implementation Plan

1. **Create `heuristic/` module structure**
   - Create `src/heuristic/mod.rs` with trait definition
   - Create `src/heuristic/standard.rs` with current implementation

2. **Move and refactor code**
   - Move `heuristic()` → `StandardHeuristic::lower_bound()`
   - Move `upper_bound_convert_then_mil()` → `StandardHeuristic::upper_bound()`
   - Update function signatures to match trait

3. **Update `lib.rs`**
   - Change `solve_and_reconstruct()` to accept heuristic parameter
   - Replace direct calls to `heuristic()` and `upper_bound_convert_then_mil()`
     with trait method calls
   - Add factory function call for Python interface

4. **Python API**
   - Add `heuristic` parameter to `solve_and_reconstruct()` (kw-only, default
     `"standard"`)
   - Use factory to create heuristic internally

5. **Testing**
   - Verify solver produces same results with refactored code
   - Test with explicit heuristic name parameter

## Benefits

1. **Extensibility**: Easy to add new heuristics by implementing trait
2. **Testability**: Each heuristic can be tested independently
3. **Clarity**: Clear separation between solver logic and heuristic logic
4. **Experimentation**: Can swap heuristics without modifying solver code
5. **Python API**: Users can experiment with different heuristics via parameter

## Future Heuristics

Potential future implementations:

- `ZeroHeuristic`: Always returns 0 (Dijkstra's algorithm)
- `GreedyHeuristic`: Optimistic estimate based on cheapest single action
- `RelaxedHeuristic`: Uses relaxed problem constraints
- `LearningHeuristic`: Uses learned cost estimates from previous runs

Each would implement the `Heuristic` trait and be selectable via the factory
function.
