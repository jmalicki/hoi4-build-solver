# RAII Refactoring Analysis: State Pool Architecture

## Executive Summary

This document analyzes approaches to simplify the `StatePool` reference counting and mapping bookkeeping by leveraging
RAII principles. The current architecture uses manual reference counting with multiple ownership sources, leading to
complex invariant checking and potential bugs.

## Current Architecture Problems

### Manual Reference Counting

The current system tracks references from three sources:

1. **`StateHandle`** (RAII) - Owns a reference via `Drop::drop()`
2. **Heap membership** - Manual `increment_ref_count()` / `decrement_ref_count()`
3. **Parent relationships** - Manual increments/decrements when parents change

### Bidirectional Mapping Complexity

Two data structures track state identity:

- `states[idx]` - Dense vector: index → state payload + metadata
- `state_to_idx` - HashMap: state payload → index (for duplicate detection)

**Invariant**: If `states[idx].ref_count > 0` and `state_to_idx.contains(states[idx].state)`, then
`state_to_idx[states[idx].state] == idx`.

**Problem**: When slots are reused, this invariant can be violated if:

- Old state value remains in `states[idx]` after freeing
- Mapping gets out of sync during slot reuse
- Complex parent relationships create intermediate states

### Current Bookkeeping Overhead

**O(n) Invariant Checks** (test-only, but still complex):

- `check_ref_count_invariants()` - Verifies all states have consistent ref counts
- `check_heap_accounting_invariants()` - Verifies heap accounting structures
- `check_free_indices_invariants()` - Verifies free list consistency

**Manual Synchronization**:

- Every `insert_state()` must update both `states[]` and `state_to_idx`
- Every `decrement_ref_count()` must remove from `state_to_idx` when ref_count reaches 0
- Every slot reuse must ensure mappings are consistent

## Proposed Approaches

### Approach A: Heap Stores `StateHandle` Instead of Indices

**Concept**: Change the heap to store `StateHandle` (or `Rc<StateHandle>`) instead of `usize` indices.

#### Implementation Details

```rust
// Current:
open: QuaternaryHeapOfIndices<usize, f64>

// Proposed:
open: PriorityQueue<StateHandle<S, T>, f64>
// or
open: PriorityQueue<Rc<StateHandle<S, T>>, f64>
```

Heap operations automatically manage ownership:

- `push(handle)` - Handle is owned by heap
- `pop()` - Returns handle, heap's ownership transferred

#### Pros

1. **Automatic Ownership**: Heap membership is automatically managed by RAII
2. **Simpler Ref Counting**: Only `StateHandle` and parent relationships need ref counting
3. **No Manual Decrement on Pop**: Heap dropping a handle automatically decrements ref count
4. **Clearer Ownership Semantics**: Type system enforces ownership transfer

#### Cons

1. **Breaking Change**: Requires changing heap implementation from index-based to value-based
2. **Loss of Dense Index Access**: Can't use stable indices for O(1) random access
3. **Heap Implementation**: `QuaternaryHeapOfIndices` is optimized for indices; would need generic priority queue
4. **Performance**:
   - Indirection: Handle → index → state (extra dereference)
   - Heap growth: Rebuilding heap with handles instead of indices might be slower
5. **Complexity**: Handles contain `*mut StatePool`, making cloning/ownership complex

#### Performance Analysis

**Current (Index-based)**:

- Heap operations: O(log n) with direct index access
- State access: `states[idx]` is O(1) with cache-friendly dense storage
- Heap rebuild: O(n) index copying

**Proposed (Handle-based)**:

- Heap operations: O(log n) but with indirection through handles
- State access: Handle → index → state (extra pointer dereference)
- Heap rebuild: O(n) handle cloning/copying (if `Clone`), or O(n) `Rc` cloning (cheap but reference counting overhead)

**Estimated Overhead**: 5-15% slower heap operations, but eliminates manual ref counting synchronization.

#### Maintainability

**Positive**:

- Fewer manual ref counting calls
- Clearer ownership semantics
- Less bookkeeping code

**Negative**:

- More complex handle lifetime management
- Requires careful handling of `*mut StatePool` in handles
- Heap implementation changes are invasive

---

### Approach B: Parents Store `StateHandle` Instead of Indices

**Concept**: Change `parent_idx: Option<NonMaxUsize>` to `parent: Option<Rc<StateHandle<S, T>>>` or use lifetime
parameters.

#### Implementation Details

```rust
// Current:
struct StateWithMetadata<S, T> {
    parent_idx: Option<NonMaxUsize>,
    // ...
}

// Option B1: Rc-based
struct StateWithMetadata<S, T> {
    parent: Option<Rc<StateHandle<S, T>>>,
    // ...
}

// Option B2: Lifetime-based (more complex)
struct StateWithMetadata<'pool, S, T> {
    parent: Option<&'pool StateHandle<S, T>>,
    // ...
}
```

#### Pros

1. **Automatic Parent Reference Counting**: Parent stays alive as long as child exists
2. **Simpler Parent Updates**: Just assign new `Rc`, old parent automatically dropped
3. **No Manual Parent Ref Counting**: No `increment_ref_count`/`decrement_ref_count` for parents

#### Cons

1. **Rc Overhead**: Reference counting overhead for parent relationships
2. **Cycle Risk**: Need to be careful about creating cycles (though unlikely in A\* search trees)
3. **Complexity**:
   - `StateWithMetadata` would need to store `Rc<StateHandle>`
   - Handles contain `*mut StatePool`, making `Rc` wrapping awkward
4. **Path Reconstruction**: Currently uses indices for fast traversal; with `Rc` might need different approach
5. **Breaking Change**: Major refactor of parent relationship handling

#### Performance Analysis

**Current (Index-based parents)**:

- Parent access: O(1) index lookup
- Parent updates: O(1) + manual ref count updates
- Path reconstruction: O(depth) index traversals

**Proposed (Rc-based parents)**:

- Parent access: O(1) `Rc` dereference (with ref counting overhead)
- Parent updates: O(1) `Rc` assignment + automatic ref count updates
- Path reconstruction: O(depth) `Rc` dereferences

**Estimated Overhead**:

- Ref counting: ~5-10% overhead per parent operation
- Memory: Extra `Rc` allocations (typically 2 pointer widths + ref count)

**Estimated Benefit**: Eliminates manual parent ref counting, reduces bugs

#### Maintainability

**Positive**:

- No manual parent ref counting
- Automatic cleanup when children are freed
- Type system prevents invalid parent references

**Negative**:

- More complex type signatures
- Need to handle `Rc` cloning carefully
- May need weak references to avoid cycles

---

### Approach C: Separate "Lifetime Registry" from "Storage"

**Concept**: Split the pool into two parts:

1. **Storage**: Dense `Vec<StateWithMetadata>` - just storage, no ref counting
2. **Registry**: `HashMap<State, Rc<StateMetadata>>` - manages lifetimes via `Rc`

#### Implementation Details

```rust
struct StateMetadata<S, T> {
    idx: usize,  // Index in storage
    ref_count: AtomicU32,  // Automatic via Rc
    cost_from_start: f64,
    parent_idx: Option<usize>,
    component_idx: Option<usize>,
    transition_info: Option<T>,
}

struct StatePool<S, T> {
    // Storage (just dense array, no lifetime management)
    states: Vec<State>,  // Just payloads, no metadata

    // Registry (manages lifetimes)
    registry: HashMap<S, Rc<StateMetadata<S, T>>>,

    // Heap stores indices (from metadata)
    open: QuaternaryHeapOfIndices<usize, f64>,
    in_open: HashSet<usize>,
    // ...
}

// Heap operations get index from Rc<StateMetadata>
// Index is stable and valid as long as Rc exists
```

#### Pros

1. **Automatic Lifetime Management**: `Rc` handles ref counting automatically
2. **No Bidirectional Mapping**: Registry is single source of truth
3. **Cleaner Separation**: Storage vs. lifetime management are separate concerns
4. **Maintains Dense Indices**: Can still use index-based heap
5. **Easier Invariant Checks**: Registry naturally maintains consistency

#### Cons

1. **Two-Level Access**: State payload in `Vec`, metadata in `Rc`
2. **Memory Overhead**: `Rc` allocations for each state (even if not in heap)
3. **Complexity**: Need to coordinate between storage and registry
4. **Performance**: Extra indirection (registry lookup) + `Rc` overhead

#### Performance Analysis

**Current (Single Structure)**:

- State access: `states[idx]` - direct O(1)
- Duplicate check: `state_to_idx.get(&state)` - O(1) hash lookup
- Total: Single lookup for both payload and metadata

**Proposed (Split Structure)**:

- State access: `registry.get(&state)?.idx` → `states[idx]` - two lookups
- Duplicate check: `registry.get(&state)` - O(1) hash lookup (same)
- Metadata access: `registry.get(&state)?` → `Rc` dereference

**Estimated Overhead**:

- Extra indirection for state access: ~10-20% slower
- `Rc` overhead: ~5% per operation
- Memory: Extra allocations for `Rc` wrappers

**Estimated Benefit**: Automatic ref counting, simpler invariants

#### Maintainability

**Positive**:

- Clear separation of concerns
- `Rc` handles ref counting automatically
- Registry is single source of truth for active states

**Negative**:

- Two data structures to keep in sync
- More complex state access patterns
- Memory management complexity (when to free storage slots?)

---

### Approach D: Remove `state_to_idx`, Use Weak References

**Concept**: Eliminate the bidirectional mapping entirely. Use weak references or a different duplicate detection
strategy.

#### Implementation Details

```rust
// Option D1: Weak references for duplicate detection
struct StatePool<S, T> {
    states: Vec<StateWithMetadata<S, T>>,
    weak_refs: HashMap<S, Weak<StateMetadata<S, T>>>,  // For duplicate detection
    // ...
}

// Option D2: Bloom filter + linear scan
struct StatePool<S, T> {
    states: Vec<StateWithMetadata<S, T>>,
    bloom_filter: BloomFilter<S>,  // Fast negative check
    // ...
}

// Option D3: Just linear scan (for small states)
struct StatePool<S, T> {
    states: Vec<StateWithMetadata<S, T>>,
    // No mapping, just scan when needed
    // ...
}
```

#### Pros

1. **No Bidirectional Mapping**: Eliminates mapping consistency problems
2. **Simpler State Management**: Only one data structure to maintain
3. **Automatic Cleanup**: Weak references automatically cleaned up

#### Cons

1. **Performance**:
   - Weak references: Need to upgrade to `Rc`, check if alive
   - Bloom filter: False positives require linear scan
   - Linear scan: O(n) duplicate checks
2. **Duplicate Detection**: Current A\* relies on fast duplicate checking
3. **Memory**: Weak references still have overhead

#### Performance Analysis

**Current (HashMap lookup)**:

- Duplicate check: O(1) average case
- Space: O(n) for HashMap

**Proposed Options**:

- **Weak refs**: O(1) lookup + O(1) upgrade check (but needs `Rc` allocations)
- **Bloom filter**: O(1) negative checks, O(n) on false positives
- **Linear scan**: O(n) always

**Estimated Impact**: Significant performance degradation for A\* search (duplicate checking is hot path)

#### Maintainability

**Positive**:

- Eliminates bidirectional mapping complexity
- Simpler data structures

**Negative**:

- Much slower duplicate detection (critical path in A\*)
- May require fundamental algorithm changes

---

## Hybrid Approaches

### Hybrid 1: Heap Ownership via RAII Wrapper

Keep indices in heap, but wrap heap entries in a RAII wrapper that manages the reference.

```rust
struct HeapEntry<S, T> {
    idx: usize,
    pool_ptr: *mut StatePool<S, T>,
}

impl<S, T> Drop for HeapEntry<S, T> {
    fn drop(&mut self) {
        unsafe { (*self.pool_ptr).decrement_ref_count(self.idx); }
    }
}

// Heap stores HeapEntry instead of usize
open: PriorityQueue<HeapEntry<S, T>, f64>
```

**Pros**: Automatic heap reference management, keeps indices **Cons**: Heap needs to be generic over entry type, complex
ownership

### Hybrid 2: Parent Indices with Automatic Ref Counting

Keep parent indices, but use a wrapper type that automatically manages ref counting.

```rust
struct ParentRef<S, T> {
    idx: usize,
    pool_ptr: *mut StatePool<S, T>,
}

impl<S, T> Drop for ParentRef<S, T> {
    fn drop(&mut self) {
        unsafe { (*self.pool_ptr).decrement_ref_count(self.idx); }
    }
}

// But stored as Option<usize>? Can't use RAII if it's just an index...
```

**Challenge**: Can't use RAII if parent is stored as just an index in metadata.

---

## Comparison Matrix

| Approach              | Performance Impact       | Maintainability | Complexity | Breaking Changes |
| --------------------- | ------------------------ | --------------- | ---------- | ---------------- |
| **A: Heap Handles**   | -5% to -15%              | +High           | High       | Major            |
| **B: Parent Handles** | -5% to -10%              | +Medium         | Medium     | Major            |
| **C: Split Registry** | -10% to -20%             | +High           | Medium     | Major            |
| **D: No Mapping**     | -50%+ (duplicate checks) | +Low            | Low        | Major            |
| **Hybrid 1**          | -5% to -10%              | +Medium         | High       | Medium           |
| **Current**           | Baseline                 | Baseline        | High       | -                |

## Recommendations

### Short-Term (Low Risk, High Value)

**Remove global invariant checks from `decrement_ref_count`**:

- Only check invariants for the specific state being decremented
- Or remove post-condition check entirely (O(n) checks are expensive)
- **Impact**: Fixes test failures, minimal risk

### Medium-Term (Medium Risk, High Value)

**Approach B: Parents via `Rc<StateHandle>`**:

- Eliminates manual parent ref counting
- Moderate refactoring
- Maintains performance characteristics
- **Trade-off**: Adds `Rc` overhead but removes complex manual bookkeeping

### Long-Term (High Risk, Transformative)

**Approach A: Heap stores handles**:

- Eliminates heap-related ref counting entirely
- Requires heap implementation changes
- Significant performance considerations
- **Best if**: Planning to change heap implementation anyway

### Not Recommended

**Approach D (No Mapping)**: Performance impact on duplicate checking is too severe for A\* search.

**Approach C (Split Registry)**: Overhead of maintaining two structures doesn't justify benefits over simpler
approaches.

## Implementation Considerations

### Migration Path

Any major refactoring should:

1. **Maintain API compatibility** - Don't break existing callers
2. **Add feature flags** - Allow gradual migration
3. **Performance benchmarks** - Measure before/after
4. **Comprehensive tests** - Ensure correctness

### Testing Strategy

For any refactoring:

1. Property-based tests for ref counting invariants
2. Performance regression tests
3. Stress tests with large state spaces
4. Fuzz testing for edge cases

## Zero-Cost Abstraction Approaches

### Generational Indices

**Concept**: Use generational indices (index + generation counter) instead of plain `usize` indices. This makes stale
indices automatically invalid without ref counting.

```rust
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct GenerationalIndex {
    index: u32,
    generation: u32,
}

struct StatePool<S, T> {
    states: Vec<StateSlot<S, T>>,
    free_list: Vec<u32>,  // Just indices, no generation
    // ...
}

struct StateSlot<S, T> {
    generation: u32,  // Incremented on reuse
    state: Option<S>,  // None when freed
    metadata: StateMetadata<T>,
}
```

**How it works**:

- Each slot has a generation counter
- When a slot is freed, increment its generation
- When slot is reused, old indices become invalid (generation mismatch)
- Heap stores `GenerationalIndex`, checks generation on access
- `StateHandle` stores `GenerationalIndex`, automatically invalid when slot reused

**Zero-cost aspects**:

- Same memory layout as `usize` (just packing generation bits)
- O(1) access, same as current
- Compile-time guarantees: can't access freed slot (generation mismatch)
- No ref counting needed for heap membership (generation check handles it)

**Eliminates**:

- Manual ref counting for heap membership
- `free_indices` list (can detect free slots by generation)
- Stale index access bugs (generation mismatch)

**Trade-off**: Still need ref counting for parent relationships (can't use generation for that).

### Typed Indices with Phantom Lifetime

**Concept**: Use type-level lifetime or type parameters to encode "this index is valid" vs "this index is freed".

```rust
struct ValidIndex<'pool> {
    idx: usize,
    _phantom: PhantomData<&'pool ()>,
}

struct FreedIndex {
    idx: usize,
}

impl<S, T> StatePool<S, T> {
    fn allocate<'a>(&'a mut self) -> ValidIndex<'a> {
        // ...
    }
}
```

**Problem**: Rust lifetimes can't represent dynamic lifetimes (ref counting is necessary).

**Alternative - Newtype with Invariant**:

```rust
#[derive(Clone, Copy)]
struct StateIndex(usize);

impl StateIndex {
    fn new(idx: usize) -> Self {
        StateIndex(idx)
    }

    fn get(&self, pool: &StatePool) -> Option<&State> {
        // Runtime check: is this index valid?
        pool.get_state(self.0)
    }
}
```

This is just encapsulation, not zero-cost.

### Borrow Checker for Pool Access

**Concept**: Use Rust's borrow checker to ensure only one place can modify the pool at a time, eliminating some
synchronization needs.

**Already using this**: The pool is `&mut self` in most methods, so borrow checker ensures exclusive access.

**Can't eliminate**: We still need ref counting because we need dynamic lifetimes (heap holds reference even when no
handles exist).

### Type-Level State Machine for Pool Slots

**Concept**: Use const generics or type-level state machine to encode slot states (Free vs Active).

```rust
enum SlotState {
    Free,
    Active,
}

struct StateSlot<State: SlotState, S, T> {
    // Different fields depending on State
}

// Specialize methods based on slot state
impl<S, T> StatePool<S, T> {
    fn allocate_free_slot(&mut self) -> Slot<Active, S, T> {
        // ...
    }
}
```

**Problem**: This would require const generics and still doesn't solve the dynamic lifetime problem.

**Better idea - Use for slot reuse detection**:

```rust
struct StateSlot<S, T> {
    state: S,
    is_active: bool,  // Single bit! Zero-cost flag
    metadata: StateMetadata<T>,
}
```

But this is just a boolean flag, not really zero-cost abstraction.

### Compile-Time Invariant Enforcement

**Concept**: Use Rust's type system to ensure invariants hold at compile time.

**The problem**: Our invariants are runtime (ref counting is dynamic).

**However**: We can use **newtypes** to prevent accidentally mixing index types or accessing freed indices:

```rust
#[derive(Clone, Copy)]
struct ActiveIndex(usize);

#[derive(Clone, Copy)]
struct FreedIndex(usize);

impl<S, T> StatePool<S, T> {
    fn get(&self, idx: ActiveIndex) -> &State {  // Can't use FreedIndex!
        &self.states[idx.0].state
    }
}
```

**Zero-cost**: Just a wrapper around usize, compiles away. Prevents bugs at compile time.

### Eliminate Bidirectional Mapping via Single Source of Truth

**Concept**: Make `state_to_idx` the ONLY place states are registered. Never query `states[]` directly for identity
checks.

```rust
impl<S, T> StatePool<S, T> {
    fn try_update_best_cost(&mut self, state: S, path_cost: f64) -> Option<NonMaxUsize> {
        // Only check state_to_idx, never states[]
        match self.state_to_idx.get(&state) {
            Some(&idx) => {
                // Update in states[idx]
                if path_cost < self.states[idx].cost_from_start {
                    self.states[idx].cost_from_start = path_cost;
                    Some(unsafe { NonMaxUsize::new_unchecked(idx) })
                } else {
                    None
                }
            }
            None => {
                // Insert new - this is the ONLY place we allocate
                let idx = self.allocate_index();
                self.states[idx].state = state.clone();
                self.state_to_idx.insert(state, idx);
                self.states[idx].cost_from_start = path_cost;
                Some(unsafe { NonMaxUsize::new_unchecked(idx) })
            }
        }
    }
}
```

**Already doing this!** The problem is the invariant check queries `states[]` by index, then looks up that state value
in `state_to_idx`.

**The fix**: Make invariant check only query `state_to_idx` (the source of truth), not `states[]`.

### RAII Wrapper for Heap Membership

**Concept**: Heap stores indices, but wraps them in a type that automatically manages ref counting via Drop.

```rust
struct HeapEntry<S, T> {
    idx: usize,
    pool: *mut StatePool<S, T>,
}

impl<S, T> Drop for HeapEntry<S, T> {
    fn drop(&mut self) {
        unsafe { (*self.pool).decrement_ref_count(self.idx); }
    }
}

// Heap stores HeapEntry instead of usize
```

**But**: Heap is `QuaternaryHeapOfIndices<usize, f64>` - it's specialized for usize. Can't change this easily.

**Alternative - Wrapper at push/pop boundary**:

```rust
impl<S, T> StatePool<S, T> {
    fn heap_push(&mut self, handle: &StateHandle<S, T>, cost: f64) {
        let idx = handle.index();
        // Create wrapper
        let entry = HeapEntry { idx, pool: self as *mut _ };
        // Store index in heap
        self.open.push(idx, -cost);
        // Entry is dropped here, but we don't want that...
    }
}
```

This doesn't work because we need the entry to live as long as it's in the heap.

**Better - RAII for heap operations**:

```rust
struct HeapOwned<S, T> {
    pool: *mut StatePool<S, T>,
    idx: usize,
}

impl<S, T> HeapOwned<S, T> {
    fn new(pool: &mut StatePool<S, T>, idx: usize) -> Self {
        pool.increment_ref_count(idx);
        HeapOwned { pool: pool as *mut _, idx }
    }
}

impl<S, T> Drop for HeapOwned<S, T> {
    fn drop(&mut self) {
        unsafe { (*self.pool).decrement_ref_count(self.idx); }
    }
}

// But heap still stores usize... hmm
```

The problem is the heap implementation is fixed to usize.

### The Real Zero-Cost Insight: Generational Indices

**This is the zero-cost abstraction we're missing!**

Generational indices:

- **Zero runtime overhead**: Same size as usize (pack generation in upper bits)
- **Zero-cost access**: Same O(1) access as current
- **Eliminates manual tracking**: Generation counter handles stale detection
- **Type safety**: GenerationalIndex type prevents using wrong generation
- **Eliminates ref counting for heap**: Heap membership validated by generation check, not ref count

**Implementation**:

```rust
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct GenerationalIndex {
    index: u32,        // 32-bit index (4B states)
    generation: u32,    // 32-bit generation
}

struct StateSlot<S, T> {
    generation: u32,
    state: S,
    metadata: StateMetadata<T>,
}

impl<S, T> StatePool<S, T> {
    fn get(&self, idx: GenerationalIndex) -> Option<&State> {
        self.states.get(idx.index as usize).and_then(|slot| {
            if slot.generation == idx.generation {
                Some(&slot.state)
            } else {
                None  // Stale index!
            }
        })
    }

    fn allocate(&mut self, state: S) -> GenerationalIndex {
        if let Some(freed_idx) = self.free_indices.pop() {
            let slot = &mut self.states[freed_idx];
            slot.generation += 1;  // Increment on reuse
            slot.state = state;
            GenerationalIndex { index: freed_idx as u32, generation: slot.generation }
        } else {
            let idx = self.states.len();
            self.states.push(StateSlot {
                generation: 0,
                state,
                metadata: Default::default(),
            });
            GenerationalIndex { index: idx as u32, generation: 0 }
        }
    }
}
```

**What this eliminates**:

- Ref counting for heap membership (generation check validates heap entries)
- Manual stale index tracking (generation mismatch = invalid)
- `free_indices` list (can detect free slots by querying generation)

**What we still need**:

- Ref counting for parent relationships (can't use generation for dynamic parent lifetime)
- `state_to_idx` for duplicate detection (still need fast lookup)

**Performance**:

- **Memory**: Same as usize (64 bits total: 32-bit index + 32-bit generation)
- **Access**: O(1) same as current, just one extra comparison (`generation == slot.generation`)
- **Allocation**: No overhead, generation increment is free
- **Zero-cost**: Generation check compiles to single `cmp` instruction

**Comparison to Current**:

- Current: Check `ref_count > 0` + manual decrement on free + tracking in `free_indices`
- Generational: Check `generation == slot.generation` + auto-increment on reuse
- **Same performance, simpler logic**

#### Why This is Zero-Cost

1. **No extra memory**: Can pack generation in same usize (use upper 32 bits)
2. **No extra indirection**: Direct comparison, not pointer chasing
3. **No heap allocations**: Everything is inline, stack-allocated
4. **Compiler optimizes away**: Generation comparison is a single CPU instruction
5. **Eliminates complex logic**: No manual ref counting, no free list tracking

#### Complete Implementation Sketch

```rust
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct GenerationalIndex(u64);  // Pack index + generation in 64 bits

impl GenerationalIndex {
    fn new(index: u32, generation: u32) -> Self {
        GenerationalIndex((index as u64) | ((generation as u64) << 32))
    }

    fn index(&self) -> usize {
        (self.0 & 0xFFFF_FFFF) as usize
    }

    fn generation(&self) -> u32 {
        (self.0 >> 32) as u32
    }
}

struct StateSlot<S, T> {
    generation: u32,
    state: S,
    cost_from_start: f64,
    parent_idx: Option<GenerationalIndex>,
    component_idx: Option<usize>,
    transition_info: Option<T>,
    // No ref_count field needed!
}

impl<S, T> StatePool<S, T> {
    fn heap_push(&mut self, handle: &StateHandle<S, T>, cost: f64) {
        let idx = handle.index();
        // No increment_ref_count! Generation check handles validity
        self.open.push(idx, -cost);
        self.in_open.insert(idx);
    }

    fn heap_pop(&mut self) -> Option<StateHandle<S, T>> {
        while let Some((idx_raw, neg_cost)) = self.open.pop() {
            let idx = GenerationalIndex::from_raw(idx_raw);
            // Check if still valid (generation matches)
            if let Some(slot) = self.states.get(idx.index()) {
                if slot.generation == idx.generation {
                    // Valid! Create handle (no ref counting needed)
                    let handle = StateHandle::new(idx, -neg_cost, self);
                    self.in_open.remove(&idx.index());
                    return Some(handle);
                }
                // Generation mismatch - stale entry, skip it
            }
        }
        None
    }
}
```

**What this eliminates completely**:

- ✅ `ref_count` field in `StateWithMetadata`
- ✅ `increment_ref_count()` / `decrement_ref_count()` for heap membership
- ✅ Manual tracking of which indices are in heap
- ✅ Complex invariant checks for ref counting
- ✅ `free_indices` list (can detect free by checking generation)

**What we still need**:

- Ref counting for parent relationships (can't use generation - parent lifetime is independent)
- `state_to_idx` for duplicate detection (still O(1) hash lookup needed)

**Performance Impact**: Zero - same memory, same access pattern, simpler logic.

## Conclusion

The current architecture's complexity stems from mixing manual ref counting with RAII. **Approach B (Parent handles via
Rc)** offers the best balance of:

- Eliminating manual bookkeeping
- Maintaining performance
- Reasonable implementation complexity

**However**, **Generational Indices** is the true zero-cost abstraction that eliminates:

- Manual ref counting for heap membership
- Stale index tracking
- Complex invariant checks for slot reuse

The ref counting would only be needed for parent relationships, which are much simpler to reason about.

However, the **immediate fix** (removing global invariant checks) should be done first to unblock development, while
planning the longer-term refactoring.

## Additional Zero-Cost Techniques

### Packing Generation in Index Bits

**Concept**: For smaller pools (< 4 billion states), can pack generation in upper bits of usize:

```rust
// Assuming max 2^32 states, use upper 32 bits for generation
struct GenerationalIndex(u64);

impl GenerationalIndex {
    fn new(index: u32, generation: u32) -> Self {
        GenerationalIndex((index as u64) | ((generation as u64) << 32))
    }

    fn index(&self) -> usize {
        (self.0 & 0xFFFF_FFFF) as usize
    }

    fn generation(&self) -> u32 {
        (self.0 >> 32) as u32
    }

    fn to_usize(&self) -> usize {
        self.0 as usize  // Can store directly in heap as usize!
    }

    fn from_usize(val: usize) -> Self {
        GenerationalIndex(val as u64)
    }
}
```

**Benefit**: Heap can still store `usize`, just interpret as `GenerationalIndex`. Zero-cost type conversion.

### Type-Level Index Validity

**Concept**: Use newtypes to prevent mixing index types, but keep zero-cost representation:

```rust
#[derive(Clone, Copy)]
struct HeapIndex(GenerationalIndex);

#[derive(Clone, Copy)]
struct ParentIndex(GenerationalIndex);

// Compiler prevents mixing types, but zero-cost at runtime
impl<S, T> StatePool<S, T> {
    fn get_heap(&self, idx: HeapIndex) -> Option<&State> {
        self.get(idx.0)  // Zero-cost wrapper
    }
}
```

**Benefit**: Type safety without runtime overhead.

### Const-Generic Pool Sizes

**Concept**: Use const generics for pool capacity, enabling compile-time optimizations:

```rust
struct StatePool<const MAX_STATES: usize, S, T> {
    states: Vec<StateSlot<S, T>>,
    // ...
}

// Compiler can unroll loops, optimize bounds checks
impl<const MAX_STATES: usize, S, T> StatePool<MAX_STATES, S, T> {
    fn get(&self, idx: GenerationalIndex) -> Option<&State> {
        if idx.index() < MAX_STATES {  // Compile-time known bound
            // ...
        }
    }
}
```

**Benefit**: Compiler optimizations, but requires const generics (Rust 1.51+).

**Trade-off**: Less flexible - need to know max size at compile time.

## Using `slotmap` Crate

### What is `slotmap`?

`slotmap` is a Rust crate that provides generational index-based storage. It uses `SlotKey` (which contains index +
generation) and provides `SlotMap` for storing data with stable, reusable keys.

### Key Components

- **`SlotKey`**: Contains index (32 bits) + generation (32 bits), packed into 64 bits
- **`SlotMap<K, V>`**: Dense storage that uses `SlotKey` for indexing
- **Automatic generation management**: Handles generation incrementing on slot reuse

### Integration with `QuaternaryHeapOfIndices`

**The Challenge**: `QuaternaryHeapOfIndices<usize, f64>` expects raw `usize` indices. It uses `with_index_bound()` to
pre-allocate a `positions` vector where `positions[idx]` maps index → heap position.

**Two Approaches**:

#### Approach 1: Store Only Index Portion in Heap

**Concept**: Extract the index portion from `SlotKey`, store that in the heap, maintain a mapping from heap entries to
full `SlotKey`.

```rust
use slotmap::{SlotKey, SlotMap, DefaultKey};

struct StatePool<S, T> {
    // Use SlotMap for state storage
    states: SlotMap<DefaultKey, StateMetadata<S, T>>,

    // Heap stores just the index portion (needs to be < heap_bound)
    open: QuaternaryHeapOfIndices<usize, f64>,

    // Mapping: heap index -> full SlotKey (for validation on pop)
    heap_keys: HashMap<usize, DefaultKey>,

    // Reverse: SlotKey -> heap index (for decrease_key)
    key_to_heap_idx: HashMap<DefaultKey, usize>,

    state_to_key: HashMap<S, DefaultKey>,  // For duplicate detection
}
```

**Problems**:

- ❌ Requires separate mapping structures (extra memory, extra lookups)
- ❌ Heap bound must match `states.len()`, but `SlotMap` doesn't expose an "index bound"
- ❌ Complex synchronization between `heap_keys` and `key_to_heap_idx`
- ❌ `SlotKey` might not fit in `usize` index bound constraints

#### Approach 2: Pack SlotKey as usize in Heap

**Concept**: Since `SlotKey` is 64 bits, can store it directly as `usize` in the heap (on 64-bit platforms). Extract
index portion when accessing heap's position vector.

```rust
use slotmap::{SlotKey, SlotMap, DefaultKey};

struct StatePool<S, T> {
    states: SlotMap<DefaultKey, StateMetadata<S, T>>,

    // Store full SlotKey as usize (64 bits = usize on 64-bit platforms)
    open: QuaternaryHeapOfIndices<usize, f64>,

    state_to_key: HashMap<S, DefaultKey>,
}

impl<S, T> StatePool<S, T> {
    fn heap_push(&mut self, key: DefaultKey, cost: f64) {
        // Convert SlotKey to usize for heap
        let key_as_usize = key.data().as_ffi() as usize;
        self.open.push(key_as_usize, -cost);
    }

    fn heap_pop(&mut self) -> Option<(DefaultKey, f64)> {
        while let Some((key_as_usize, neg_cost)) = self.open.pop() {
            // Reconstruct SlotKey from usize
            let key = DefaultKey::from_ffi(key_as_usize as u64);

            // Validate: check if key still exists in SlotMap
            if self.states.contains_key(key) {
                return Some((key, -neg_cost));
            }
            // Stale entry (slot was reused), skip it
        }
        None
    }
}
```

**Problems**:

- ❌ **Heap bound constraint**: `QuaternaryHeapOfIndices::with_index_bound(n)` requires indices `< n`, but `SlotKey`
  values can be much larger
- ❌ **Heap position mapping**: The heap uses `positions[idx]` internally, but `SlotKey` isn't a dense 0..n range
- ❌ Heap's internal `positions` vector would need to accommodate full `SlotKey` range (memory overhead)

#### Approach 3: Custom Generational Index (Not Using slotmap)

**Concept**: Implement our own generational indices, designed to work with the heap's constraints.

```rust
// Custom generational index that fits heap constraints
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct StateIndex {
    // For heap: store just index (u32), must be < heap_bound
    index: u32,
    generation: u32,
}

// Slot stores generation
struct StateSlot<S, T> {
    generation: u32,
    state: S,
    // ...
}

// Heap stores just index (u32), we validate generation separately
impl<S, T> StatePool<S, T> {
    fn heap_push(&mut self, idx: StateIndex, cost: f64) {
        assert!(idx.index() < self.heap_bound as u32);
        // Store just index in heap (generation check happens on access)
        self.open.push(idx.index() as usize, -cost);
        // Store full index for validation
        self.heap_indices.insert(idx.index() as usize, idx);
    }

    fn heap_pop(&mut self) -> Option<(StateIndex, f64)> {
        while let Some((index_u32, neg_cost)) = self.open.pop() {
            let stored_idx = self.heap_indices.remove(&index_u32)?;
            // Validate generation
            if let Some(slot) = self.states.get(index_u32) {
                if slot.generation == stored_idx.generation() {
                    return Some((stored_idx, -neg_cost));
                }
            }
            // Stale, skip
        }
        None
    }
}
```

**Advantages**:

- ✅ **Fits heap constraints**: Index portion is u32, works with `index_bound`
- ✅ **No external dependency**: Full control over implementation
- ✅ **Zero-cost**: Can pack index + generation in usize when needed

**Trade-offs**:

- ❌ Need to implement slot reuse logic ourselves
- ❌ Miss out on slotmap's well-tested implementation
- ❌ Still need mapping structure for heap validation

### Recommendation

**Don't use `slotmap` directly** because:

1. **Heap constraint incompatibility**: `QuaternaryHeapOfIndices` requires dense indices `0..n` with a fixed bound.
   `SlotKey` doesn't fit this pattern.

2. **Memory overhead**: Would need extra mappings to translate between `SlotKey` and heap index positions.

3. **Complexity**: More complex than implementing generational indices ourselves, which need to match heap constraints.

**Instead**: Implement custom generational indices that:

- Store index as `u32` (fits in `index_bound` constraint)
- Pack index + generation when needed outside heap
- Validate generation on heap operations
- Use heap's index directly, maintain generation separately

This gives us:

- ✅ Zero-cost generational validation
- ✅ Compatibility with existing heap
- ✅ Simpler than integrating slotmap
- ✅ Full control over slot reuse

## Questions for Further Analysis

1. **What is the maximum number of states in a typical run?** (Determines if we can pack generation in upper bits)
2. **How frequently do parent relationships change?** (Impacts whether Rc for parents is worth it)
3. **What is the typical heap size vs total states?** (Impacts heap ref counting overhead)
4. **How common are duplicate states in A\*?** (Impacts duplicate detection performance needs)
5. **Can we eliminate parent ref counting too?** (Maybe parents don't need ref counting if they're always in heap?)
