# Implementation Plan: Generational Slot Storage

## Overview

This document provides a detailed implementation plan for introducing generational indices to replace manual reference
counting in the state pool. Each phase will be implemented as a separate stacked PR.

## Repository Structure

The generational slots implementation will live in:

- **Location**: `src/hoi4_build_core/src/state_pool/generational_slots.rs`
- **Documentation**: `src/hoi4_build_core/src/state_pool/generational_slots/README.md` (if needed)

## Branch Strategy

All work will be done on feature branches off `main`:

- Base branch: `main`
- Branch naming: `feature/generational-slots-phase-{N}`
- Each phase is a separate PR that can be merged independently
- PRs are stacked (later phases depend on earlier ones)

## Pre-Implementation Checklist

Before starting:

- [ ] Ensure all tests pass on `main`: `cd src/hoi4_build_core && cargo test`
- [ ] Ensure code is formatted: `cd src/hoi4_build_core && cargo fmt`
- [ ] Ensure no linter errors: `cd src/hoi4_build_core && cargo clippy`
- [ ] Read `personas/ai-developer-standards.md` for git workflow
- [ ] Understand Conventional Commits format

---

## Phase 1: Core GenerationalIndex Type

**Goal**: Implement the `GenerationalIndex` type and basic utilities.

**Branch**: `feature/generational-slots-phase-1`

### Implementation Steps

1. **Create new module file**

   ```bash
   cd src/hoi4_build_core/src/state_pool
   touch generational_slots.rs
   ```

2. **Implement GenerationalIndex**
   - Copy design from `docs/DESIGN_GENERATIONAL_SLOTS.md`
   - Implement all methods: `new()`, `index()`, `generation()`, `to_usize()`, `from_usize()`, `is_valid_for()`
   - Add `#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]`
   - Add comprehensive doc comments

3. **Add to module tree**
   - Add `pub mod generational_slots;` to `src/state_pool/mod.rs`
   - Export `GenerationalIndex` from the module

4. **Write unit tests**
   - Test `new()`, `index()`, `generation()`
   - Test `to_usize()` / `from_usize()` roundtrip
   - Test `is_valid_for()` with matching/mismatching generations
   - Test edge cases (max u32 values, zero generation)

5. **Run checks**
   ```bash
   cd src/hoi4_build_core
   cargo fmt
   cargo clippy --all-targets --all-features -- -D warnings
   cargo test --lib generational_slots
   ```

### Deliverables

- [ ] `GenerationalIndex` type implemented
- [ ] All unit tests pass
- [ ] Code formatted and linted
- [ ] No warnings

### PR Details

**Title**: `feat(state_pool): add GenerationalIndex type`

**Description**:

```markdown
Add the core GenerationalIndex type for generational slot storage.

This is the first phase of migrating to generational indices to replace manual reference counting. The GenerationalIndex
type packs an index (u32) and generation counter (u32) into a single type that can be zero-cost validated.

Part of refactoring to eliminate complex ref counting bookkeeping. See docs/DESIGN_GENERATIONAL_SLOTS.md for full
design.

No functional changes yet - this is infrastructure only.
```

**Testing**:

- Unit tests for `GenerationalIndex`
- All existing tests still pass

---

## Phase 2: GenerationalSlotStorage Core

**Goal**: Implement the `GenerationalSlotStorage` structure with allocate/free/get operations.

**Branch**: `feature/generational-slots-phase-2`

**Base**: After Phase 1 is merged (or branched from Phase 1 branch)

### Implementation Steps

1. **Implement Slot<T> and SlotMetadata**
   - Copy from design document
   - Keep `SlotMetadata` simple for now (can expand later)

2. **Implement GenerationalSlotStorage<T>**
   - Implement `new()` constructor
   - Implement `allocate()` - reuse free slots, increment generation
   - Implement `free()` - validate generation, return data
   - Implement `get()` / `get_mut()` - validate generation
   - Implement `get_metadata()` / `get_metadata_mut()`
   - Implement `is_valid()` - generation check
   - Implement capacity/free_count/active_count helpers

3. **Write comprehensive tests**
   - Test allocation of new slots
   - Test free and reuse (generation increments)
   - Test stale index detection (old index after reuse)
   - Test capacity growth
   - Test metadata access
   - Test edge cases (free already-freed slot, get invalid index)

4. **Run checks**
   ```bash
   cd src/hoi4_build_core
   cargo fmt
   cargo clippy --all-targets --all-features -- -D warnings
   cargo test --lib generational_slots
   ```

### Deliverables

- [ ] `GenerationalSlotStorage` fully implemented
- [ ] All unit tests pass (100% coverage of new code)
- [ ] Code formatted and linted
- [ ] No warnings

### PR Details

**Title**: `feat(state_pool): implement GenerationalSlotStorage`

**Description**:

```markdown
Implement the core GenerationalSlotStorage structure.

This provides generational slot allocation and management. Key features:

- Automatic generation incrementing on slot reuse
- Zero-cost stale index detection via generation comparison
- Dense storage that fits heap's index_bound constraints

See docs/DESIGN_GENERATIONAL_SLOTS.md for design details.

Still not integrated with StatePool - this is the infrastructure layer.
```

**Testing**:

- Comprehensive unit tests for storage operations
- All existing tests still pass

---

## Phase 3: StatePool Integration (Behind Feature Flag)

**Goal**: Integrate generational storage with StatePool behind a feature flag, keep old code working.

**Branch**: `feature/generational-slots-phase-3`

**Base**: After Phase 2 is merged

### Implementation Steps

1. **Add feature flag to Cargo.toml**

   ```toml
   [features]
   default = []
   generational-slots = []
   ```

2. **Create StatePool implementation using generational storage**
   - Add `#[cfg(feature = "generational-slots")]` version of `StatePool`
   - Keep original version with `#[cfg(not(feature = "generational-slots"))]`
   - Or use a generic parameter / trait to switch implementations
   - Actually, simpler: use type alias with feature flag

3. **Implementation approach**:

   ```rust
   #[cfg(not(feature = "generational-slots"))]
   type StatePoolImpl = StatePoolOld;

   #[cfg(feature = "generational-slots")]
   type StatePoolImpl = StatePoolNew;

   // Or use a generic parameter
   ```

4. **Actually, better approach - implement alongside existing code**:
   - Add `storage: GenerationalSlotStorage<StateSlot<S, T>>` field
   - Add `heap_indices: HashMap<usize, GenerationalIndex>` for heap tracking
   - Keep existing `states: Vec<StateWithMetadata<S, T>>` behind feature flag
   - Implement new methods that use generational storage
   - Keep old methods with `#[cfg(not(feature = "generational-slots"))]`

5. **Integrate heap operations**
   - Modify `heap_push()` to use `GenerationalIndex`
   - Modify `heap_pop()` to validate generation and skip stale entries
   - Add `heap_indices` HashMap to track full indices in heap

6. **Update duplicate detection**
   - Modify `try_update_best_cost()` to use generational storage
   - Update `state_to_idx` to map to `GenerationalIndex`

7. **Write integration tests**
   - Test heap operations with generational indices
   - Test stale entry skipping in heap_pop
   - Test duplicate detection still works
   - Test parent relationships (may still need ref counting for now)

8. **Run checks**

   ```bash
   cd src/hoi4_build_core

   # Test without feature
   cargo test --lib
   cargo fmt
   cargo clippy --all-targets -- -D warnings

   # Test with feature
   cargo test --features generational-slots --lib
   cargo clippy --features generational-slots --all-targets -- -D warnings
   ```

### Deliverables

- [ ] Feature flag implementation
- [ ] Both old and new code work
- [ ] Integration tests pass
- [ ] All existing tests pass (both with and without feature)
- [ ] Code formatted and linted

### PR Details

**Title**: `feat(state_pool): integrate generational storage behind feature flag`

**Description**:

```markdown
Integrate GenerationalSlotStorage with StatePool behind feature flag.

This allows testing the new implementation side-by-side with the old one. The feature flag `generational-slots` enables
the new code path.

Changes:

- Add generational storage alongside existing storage
- Update heap operations to use GenerationalIndex
- Add heap_indices HashMap for tracking full indices in heap
- Implement stale entry skipping in heap_pop

Testing:

- All tests pass without feature flag (old code)
- All tests pass with feature flag (new code)
- Integration tests verify heap operations work correctly

Part of phased migration to eliminate ref counting complexity.
```

**Testing**:

- Both code paths tested
- Integration tests for heap operations
- Existing tests still pass

---

## Phase 4: Remove Ref Counting for Heap Membership

**Goal**: Eliminate ref counting for heap membership, rely solely on generation validation.

**Branch**: `feature/generational-slots-phase-4`

**Base**: After Phase 3 is merged

### Implementation Steps

1. **Remove heap ref counting**
   - Remove `increment_ref_count()` calls in `heap_push()`
   - Remove `decrement_ref_count()` calls in `heap_pop()`
   - Remove ref counting for heap membership entirely
   - Keep ref counting for parent relationships (still needed)

2. **Update invariant checks**
   - Modify `check_ref_count_invariants()` to not require heap refs
   - Update `check_heap_accounting_invariants()` if needed
   - Remove ref count assertions from heap methods

3. **Update tests**
   - Remove tests that verify heap ref counting
   - Add tests that verify generation validation in heap operations
   - Test stale entry skipping works correctly

4. **Run checks**
   ```bash
   cd src/hoi4_build_core
   cargo fmt
   cargo clippy --features generational-slots --all-targets -- -D warnings
   cargo test --features generational-slots --lib
   ```

### Deliverables

- [ ] No ref counting for heap membership
- [ ] Generation validation handles heap entry validity
- [ ] All tests pass
- [ ] Code formatted and linted

### PR Details

**Title**: `feat(state_pool): eliminate ref counting for heap membership`

**Description**:

```markdown
Remove manual ref counting for heap membership, use generation validation instead.

Changes:

- Remove increment_ref_count() / decrement_ref_count() for heap operations
- Heap entries validated via generation check on pop
- Stale heap entries automatically skipped

This simplifies the code significantly - heap membership is now validated automatically via generational indices rather
than manual ref counting.

Ref counting still needed for parent relationships (independent lifetimes).
```

**Testing**:

- Tests verify heap operations work without ref counting
- Tests verify stale entries are skipped
- All existing functionality preserved

---

## Phase 5: Simplify Parent Ref Counting (Optional)

**Goal**: Evaluate and potentially simplify parent ref counting using generational indices.

**Branch**: `feature/generational-slots-phase-5`

**Base**: After Phase 4 is merged

### Implementation Steps

1. **Analyze parent relationship lifetimes**
   - Determine if parents are always in heap when children reference them
   - If yes, can eliminate parent ref counting (generation validates)
   - If no, may still need ref counting or different strategy

2. **If eliminating is possible**:
   - Remove parent ref counting
   - Use generation validation instead
   - Update tests

3. **If not possible**:
   - Document why ref counting is still needed
   - Consider if `Rc<StateHandle>` would be simpler (see RAII_REFACTORING_ANALYSIS.md)

4. **Run checks**
   ```bash
   cd src/hoi4_build_core
   cargo fmt
   cargo clippy --features generational-slots --all-targets -- -D warnings
   cargo test --features generational-slots --lib
   ```

### Deliverables

- [ ] Analysis of parent relationship lifetimes
- [ ] Either: ref counting removed, or: documentation of why it's needed
- [ ] All tests pass
- [ ] Code formatted and linted

### PR Details

**Title**: `refactor(state_pool): simplify parent ref counting with generational indices` (or
`docs(state_pool): document parent ref counting requirements` if not feasible)

**Description**: (Will depend on analysis results)

---

## Phase 6: Remove Feature Flag, Make Default

**Goal**: Make generational slots the default implementation.

**Branch**: `feature/generational-slots-phase-6`

**Base**: After Phase 4 (or 5) is merged

### Implementation Steps

1. **Make generational slots default**
   - Change feature flag to `legacy-ref-counting` (inverted)
   - Or remove feature flag entirely, keep old code for comparison
   - Actually, best: remove old code entirely

2. **Remove old implementation**
   - Delete `ref_count` field from `StateWithMetadata`
   - Delete `increment_ref_count()` / `decrement_ref_count()` (except for parents if still needed)
   - Delete old `free_indices` management
   - Clean up code that's no longer needed

3. **Update all tests**
   - Remove `#[cfg(feature = "...")]` guards
   - Update tests to use generational indices
   - Remove tests that verify old behavior

4. **Update documentation**
   - Update inline docs to reflect new implementation
   - Update `pool.rs` module documentation
   - Remove references to ref counting for heap

5. **Run checks**

   ```bash
   cd src/hoi4_build_core
   cargo fmt
   cargo clippy --all-targets --all-features -- -D warnings
   cargo test --all-targets --all-features

   # Also run Python tests if they exist
   cd ../..
   cargo test
   ```

### Deliverables

- [ ] Feature flag removed (or inverted)
- [ ] Old code removed
- [ ] All tests pass
- [ ] Documentation updated
- [ ] Code formatted and linted

### PR Details

**Title**: `feat(state_pool): make generational slots the default implementation`

**Description**:

```markdown
Remove feature flag and make generational slots the default implementation.

Changes:

- Remove generational-slots feature flag
- Remove old ref counting implementation for heap membership
- Clean up unused code (ref_count field, increment/decrement methods)
- Update all tests and documentation

This completes the migration to generational indices, eliminating the complex ref counting bookkeeping for heap
membership.

Breaking changes: None (internal implementation change only).
```

**Testing**:

- All tests pass
- Python integration tests pass (if applicable)
- Performance benchmarks (if any) verify no regression

---

## Phase 7: Cleanup and Documentation

**Goal**: Final cleanup, update all documentation, add examples.

**Branch**: `feature/generational-slots-phase-7`

**Base**: After Phase 6 is merged

### Implementation Steps

1. **Update module documentation**
   - Update `pool.rs` module-level docs
   - Add examples showing generational index usage
   - Document the heap integration pattern

2. **Add README for generational_slots module** (if complex enough)
   - Document the design decisions
   - Show usage examples
   - Explain zero-cost aspects

3. **Update DESIGN_GENERATIONAL_SLOTS.md**
   - Mark as "Implemented"
   - Add notes on what changed during implementation
   - Document any design decisions made during implementation

4. **Update RAII_REFACTORING_ANALYSIS.md**
   - Mark generational indices approach as "Implemented"
   - Note performance results
   - Document lessons learned

5. **Final code review**
   - Run full test suite
   - Check for any remaining TODO comments
   - Ensure all doc comments are complete
   - Verify no dead code

6. **Run final checks**
   ```bash
   cd src/hoi4_build_core
   cargo fmt
   cargo clippy --all-targets --all-features -- -D warnings
   cargo test --all-targets --all-features
   cargo doc --no-deps --document-private-items
   ```

### Deliverables

- [ ] All documentation updated
- [ ] Examples added
- [ ] Design documents updated
- [ ] No TODO comments
- [ ] All tests pass
- [ ] Documentation builds without warnings

### PR Details

**Title**: `docs(state_pool): finalize generational slots documentation and cleanup`

**Description**:

```markdown
Complete documentation and cleanup for generational slots implementation.

Changes:

- Update all module documentation
- Add usage examples
- Update design documents
- Remove any remaining TODOs
- Final code review and cleanup

This is the final phase of the generational slots migration.
```

**Testing**:

- All tests pass
- Documentation builds successfully
- No warnings

---

## PR Creation Workflow

For each phase:

1. **Create branch from base**

   ```bash
   git checkout main  # or previous phase branch
   git pull origin main  # Ensure up to date
   git checkout -b feature/generational-slots-phase-{N}
   ```

2. **Implement changes**
   - Make commits following Conventional Commits format
   - Example: `feat(state_pool): implement GenerationalSlotStorage`
   - Use atomic commits (one logical change per commit)

3. **Run pre-commit checks**

   ```bash
   cd src/hoi4_build_core
   cargo fmt
   cargo clippy --all-targets --all-features -- -D warnings
   cargo test --all-targets --all-features
   ```

4. **Push branch**

   ```bash
   git push origin feature/generational-slots-phase-{N}
   ```

5. **Create PR on GitHub**
   - Title: Use format specified in each phase
   - Description: Copy from phase's "PR Details" section
   - Base branch: `main` (or previous phase branch if stacking)
   - Add labels: `enhancement`, `refactoring` (if applicable)

6. **Request review**
   - Mention it's part of stacked PR series
   - Link to previous phases if applicable
   - Note which tests were added/updated

7. **After approval, merge**
   - Merge commit (or rebase-merge based on project preference)
   - Delete branch after merge

## Testing Strategy

### Unit Tests

Each phase should have:

- **New code**: 100% test coverage
- **Existing code**: All existing tests still pass
- **Integration tests**: Test new code with existing code

### Test Commands

```bash
# Run all tests
cd src/hoi4_build_core && cargo test --all-targets --all-features

# Run specific module tests
cargo test --lib state_pool::generational_slots

# Run with feature flag
cargo test --features generational-slots --lib

# Run without feature flag
cargo test --lib  # Should default to old implementation

# Run tests with verbose output
cargo test --lib -- --nocapture --test-threads=1
```

### Performance Testing

Consider adding benchmarks:

```rust
#[cfg(test)]
mod benches {
    use super::*;
    use criterion::{black_box, criterion_group, criterion_main, Criterion};

    fn bench_allocate(c: &mut Criterion) {
        // Benchmark allocation speed
    }
}
```

## Code Quality Checklist

Before each PR:

- [ ] All tests pass
- [ ] `cargo fmt` run (no changes)
- [ ] `cargo clippy` passes (no warnings)
- [ ] All doc comments complete
- [ ] No unsafe code (unless documented and necessary)
- [ ] No `#[allow(...)]` without justification
- [ ] No dead code
- [ ] No unused imports
- [ ] Commit messages follow Conventional Commits
- [ ] Branch is up to date with base

## Rollback Plan

If issues arise:

1. **Phase 1-2**: Can revert easily (new code, not integrated)
2. **Phase 3-6**: Feature flag allows reverting to old code
3. **Phase 7**: After merge, can revert entire feature via git revert

Each phase should be independently reversible.

## Success Criteria

Migration is successful when:

- [ ] All tests pass with generational slots
- [ ] No performance regression (benchmark if possible)
- [ ] Code complexity reduced (fewer manual ref counting calls)
- [ ] Invariant checks simplified or removed
- [ ] Documentation complete
- [ ] No breaking changes to public API

## Timeline Estimate

- **Phase 1**: 1-2 hours (simple type)
- **Phase 2**: 2-3 hours (storage implementation)
- **Phase 3**: 3-4 hours (integration, feature flag)
- **Phase 4**: 1-2 hours (remove ref counting)
- **Phase 5**: 1-2 hours (optional, analyze parents)
- **Phase 6**: 2-3 hours (remove feature flag)
- **Phase 7**: 1-2 hours (documentation)

**Total**: ~11-18 hours of development time, plus review time.

## Notes

- Each phase should be small enough for easy review
- PRs can be merged independently (stacking is okay)
- Tests should verify both old and new behavior during migration
- Keep old code until Phase 6 to enable easy rollback
