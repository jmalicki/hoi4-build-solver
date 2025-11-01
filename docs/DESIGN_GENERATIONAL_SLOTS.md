# Generational Slot Storage Design

**Status**: Design phase. Implementation plan in `IMPLEMENTATION_PLAN_GENERATIONAL_SLOTS.md`.

## Overview

**Location**: This will be implemented in `src/hoi4_build_core/src/state_pool/generational_slots.rs` as a new module.

A zero-cost generational slot storage system designed to work with `QuaternaryHeapOfIndices`. Provides stable, reusable
indices with automatic stale detection, eliminating the need for manual reference counting for heap membership.

## Core Design

### GenerationalIndex

The key type that encapsulates index + generation:

```rust
/// A generational index that combines a slot index with a generation counter.
///
/// This allows detecting stale indices when slots are reused. The index portion
/// must fit within the heap's `index_bound` constraint (u32 is sufficient for
/// 4 billion states).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct GenerationalIndex {
    /// The slot index (lower 32 bits) - must be < heap_bound
    index: u32,
    /// The generation counter (upper 32 bits) - increments on slot reuse
    generation: u32,
}

impl GenerationalIndex {
    /// Create a new generational index.
    pub fn new(index: u32, generation: u32) -> Self {
        GenerationalIndex { index, generation }
    }

    /// Extract the index portion (for heap operations).
    pub fn index(&self) -> usize {
        self.index as usize
    }

    /// Get the generation counter.
    pub fn generation(&self) -> u32 {
        self.generation
    }

    /// Pack into usize (for storing in HashMap, etc).
    /// On 64-bit platforms, this fits perfectly.
    pub fn to_usize(&self) -> usize {
        ((self.generation as u64) << 32) | (self.index as u64) as usize
    }

    /// Unpack from usize.
    pub fn from_usize(val: usize) -> Self {
        GenerationalIndex {
            index: (val & 0xFFFF_FFFF) as u32,
            generation: ((val >> 32) & 0xFFFF_FFFF) as u32,
        }
    }

    /// Check if this index is valid for the given slot.
    pub fn is_valid_for(&self, slot_generation: u32) -> bool {
        self.generation == slot_generation
    }
}
```

### Slot&lt;T&gt;

A single slot in the storage:

```rust
/// A slot in the generational storage.
struct Slot<T> {
    /// The generation counter. Incremented each time this slot is reused.
    generation: u32,

    /// The actual data stored in this slot.
    /// `None` when the slot is free.
    data: Option<T>,

    /// Additional metadata (can be customized per use case).
    metadata: SlotMetadata,
}

/// Per-slot metadata (customizable).
#[derive(Clone, Default)]
struct SlotMetadata {
    // Add fields as needed:
    // - cost_from_start: f64,
    // - parent_idx: Option<GenerationalIndex>,
    // - component_idx: Option<usize>,
    // - transition_info: Option<T>,
}
```

### GenerationalSlotStorage

The main storage structure:

```rust
/// Generational slot storage that provides stable, reusable indices.
///
/// This is designed to work with `QuaternaryHeapOfIndices` by ensuring:
/// 1. Indices are dense in range `0..len()` (fits heap's `index_bound`)
/// 2. Slot reuse increments generation (stale indices are detected automatically)
/// 3. Zero-cost validation (single generation comparison)
pub struct GenerationalSlotStorage<T> {
    /// Dense vector of slots.
    slots: Vec<Slot<T>>,

    /// Free list of indices available for reuse.
    /// Stores just the index (u32), generation is in the slot itself.
    free_list: Vec<u32>,

    /// Current capacity (matches slots.len()).
    capacity: usize,
}
```

## API Design

### Basic Operations

```rust
impl<T> GenerationalSlotStorage<T> {
    /// Create a new storage with initial capacity.
    pub fn new(initial_capacity: usize) -> Self {
        GenerationalSlotStorage {
            slots: Vec::with_capacity(initial_capacity),
            free_list: Vec::new(),
            capacity: 0,
        }
    }

    /// Allocate a new slot and return its generational index.
    ///
    /// If a free slot is available, reuses it (incrementing generation).
    /// Otherwise, allocates a new slot.
    pub fn allocate(&mut self, data: T) -> GenerationalIndex {
        if let Some(freed_idx) = self.free_list.pop() {
            // Reuse freed slot
            let slot = &mut self.slots[freed_idx as usize];
            slot.generation += 1;  // Increment on reuse
            slot.data = Some(data);
            slot.metadata = SlotMetadata::default();

            GenerationalIndex::new(freed_idx, slot.generation)
        } else {
            // Allocate new slot
            let idx = self.slots.len();
            self.slots.push(Slot {
                generation: 0,
                data: Some(data),
                metadata: SlotMetadata::default(),
            });
            self.capacity = self.slots.len();

            GenerationalIndex::new(idx as u32, 0)
        }
    }

    /// Free a slot by index, returning the stored data.
    ///
    /// # Returns
    /// - `Some(data)` if the index was valid and slot was active
    /// - `None` if index is out of bounds or already freed (generation mismatch)
    pub fn free(&mut self, idx: GenerationalIndex) -> Option<T> {
        let slot_idx = idx.index();
        if slot_idx >= self.slots.len() {
            return None;
        }

        let slot = &mut self.slots[slot_idx];

        // Check generation - if mismatch, slot was already reused
        if slot.generation != idx.generation() {
            return None;  // Stale index
        }

        // Free the slot
        if slot.data.is_none() {
            return None;  // Already free
        }

        let data = slot.data.take()?;
        slot.metadata = SlotMetadata::default();
        self.free_list.push(idx.index() as u32);

        Some(data)
    }

    /// Get a reference to the data at an index.
    ///
    /// Returns `None` if index is invalid (out of bounds or generation mismatch).
    pub fn get(&self, idx: GenerationalIndex) -> Option<&T> {
        let slot_idx = idx.index();
        self.slots.get(slot_idx).and_then(|slot| {
            if slot.generation == idx.generation() {
                slot.data.as_ref()
            } else {
                None  // Stale index
            }
        })
    }

    /// Get a mutable reference to the data at an index.
    pub fn get_mut(&mut self, idx: GenerationalIndex) -> Option<&mut T> {
        let slot_idx = idx.index();
        self.slots.get_mut(slot_idx).and_then(|slot| {
            if slot.generation == idx.generation() {
                slot.data.as_mut()
            } else {
                None  // Stale index
            }
        })
    }

    /// Get a reference to the slot metadata.
    pub fn get_metadata(&self, idx: GenerationalIndex) -> Option<&SlotMetadata> {
        let slot_idx = idx.index();
        self.slots.get(slot_idx).and_then(|slot| {
            if slot.generation == idx.generation() {
                Some(&slot.metadata)
            } else {
                None
            }
        })
    }

    /// Get a mutable reference to the slot metadata.
    pub fn get_metadata_mut(&mut self, idx: GenerationalIndex) -> Option<&mut SlotMetadata> {
        let slot_idx = idx.index();
        self.slots.get_mut(slot_idx).and_then(|slot| {
            if slot.generation == idx.generation() {
                Some(&mut slot.metadata)
            } else {
                None
            }
        })
    }

    /// Check if an index is valid (not stale).
    pub fn is_valid(&self, idx: GenerationalIndex) -> bool {
        let slot_idx = idx.index();
        self.slots.get(slot_idx).map_or(false, |slot| {
            slot.generation == idx.generation() && slot.data.is_some()
        })
    }

    /// Current capacity (maximum index + 1).
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Number of free slots available for reuse.
    pub fn free_count(&self) -> usize {
        self.free_list.len()
    }

    /// Number of active slots.
    pub fn active_count(&self) -> usize {
        self.capacity - self.free_list.len()
    }
}
```

## Integration with StatePool

### Integration Points

```rust
use super::generational_slots::{GenerationalSlotStorage, GenerationalIndex, SlotMetadata};

struct StatePool<S, T> {
    // State storage with generational indices
    storage: GenerationalSlotStorage<StateSlot<S, T>>,

    // Duplicate detection: state value -> generational index
    state_to_idx: HashMap<S, GenerationalIndex>,

    // Heap: stores just index portion (u32), validates generation on pop
    open: QuaternaryHeapOfIndices<usize, f64>,

    // Track which indices are in heap (for decrease_key)
    in_open: HashSet<usize>,

    // Heap accounting
    heap_bound: usize,
    heap_sum_estimated_total_cost: f64,
    heap_len: usize,
}

struct StateSlot<S, T> {
    state: S,
    cost_from_start: f64,
    parent_idx: Option<GenerationalIndex>,
    component_idx: Option<usize>,
    transition_info: Option<T>,
}

// Store state metadata in the slot data, not separate metadata
impl<S, T> StatePool<S, T> {
    fn try_update_best_cost(&mut self, state: S, path_cost: f64) -> Option<GenerationalIndex> {
        match self.state_to_idx.get(&state) {
            Some(&idx) => {
                // State exists: check if path is better
                if let Some(slot) = self.storage.get_mut(idx) {
                    if path_cost < slot.cost_from_start {
                        slot.cost_from_start = path_cost;
                        Some(idx)
                    } else {
                        None  // Existing path is better
                    }
                } else {
                    None  // Stale index (shouldn't happen if state_to_idx is maintained)
                }
            }
            None => {
                // New state: allocate slot
                let slot = StateSlot {
                    state: state.clone(),
                    cost_from_start: path_cost,
                    parent_idx: None,
                    component_idx: None,
                    transition_info: None,
                };
                let idx = self.storage.allocate(slot);
                self.state_to_idx.insert(state, idx);
                Some(idx)
            }
        }
    }

    fn heap_push(&mut self, idx: GenerationalIndex, cost: f64) {
        let index_u32 = idx.index();

        // Grow heap if needed (same logic as current)
        if index_u32 >= self.heap_bound {
            // ... grow heap ...
        }

        // No ref counting! Just push index
        self.open.push(index_u32, -cost);
        self.in_open.insert(index_u32);
        // ... update accounting ...
    }

    fn heap_pop(&mut self) -> Option<GenerationalIndex> {
        while let Some((index_u32, neg_cost)) = self.open.pop() {
            // Reconstruct generational index - need to look up generation
            // Options:
            // 1. Store full GenerationalIndex in separate mapping
            // 2. Reconstruct from state_to_idx (slower)
            // 3. Store generation in heap metadata somehow

            // For now, simplest: maintain a mapping
            if let Some(&full_idx) = self.heap_indices.get(&index_u32) {
                // Validate generation
                if self.storage.is_valid(full_idx) {
                    self.in_open.remove(&index_u32);
                    return Some(full_idx);
                }
                // Stale entry (slot was reused), skip it
            }
        }
        None
    }
}
```

## Design Decisions

### Why Separate from slotmap?

1. **Heap Constraint**: `QuaternaryHeapOfIndices` requires dense indices in `0..n` range with a fixed bound. `slotmap`'s
   `SlotKey` doesn't fit this constraint.

2. **Simpler Integration**: Can design API specifically for our heap use case.

3. **Zero External Dependencies**: One less dependency, full control over implementation.

4. **Custom Metadata**: Can embed metadata directly in slots rather than separate structures.

### Why Store Full GenerationalIndex in Heap?

**Problem**: Heap stores just `usize` (the index portion), but we need the generation to validate on pop.

**Solutions**:

#### Option 1: Separate Heap Mapping (Recommended)

```rust
// Map: heap index -> full GenerationalIndex
heap_indices: HashMap<usize, GenerationalIndex>,

fn heap_push(&mut self, idx: GenerationalIndex, cost: f64) {
    let index_u32 = idx.index();
    self.open.push(index_u32, -cost);
    self.heap_indices.insert(index_u32, idx);  // Store full index
    // ...
}

fn heap_pop(&mut self) -> Option<GenerationalIndex> {
    while let Some((index_u32, _)) = self.open.pop() {
        if let Some(full_idx) = self.heap_indices.remove(&index_u32) {
            if self.storage.is_valid(full_idx) {
                return Some(full_idx);
            }
        }
    }
    None
}
```

**Pros**: Simple, clear separation **Cons**: Extra HashMap (but heap size is typically small compared to total states)

#### Option 2: Reconstruct from state_to_idx

```rust
fn heap_pop(&mut self) -> Option<GenerationalIndex> {
    while let Some((index_u32, _)) = self.open.pop() {
        // Find the state at this index (linear scan of state_to_idx)
        // Check generation
        // ...
    }
}
```

**Pros**: No extra mapping **Cons**: O(n) lookup, very slow

#### Option 3: Store Generation in Separate Vector

```rust
// Parallel to slots: heap_generations[i] = generation of slot i when pushed
heap_generations: Vec<u32>,

fn heap_push(&mut self, idx: GenerationalIndex, cost: f64) {
    let index_u32 = idx.index();
    // Resize if needed
    if index_u32 >= self.heap_generations.len() {
        self.heap_generations.resize(index_u32 + 1, 0);
    }
    self.heap_generations[index_u32] = idx.generation();
    self.open.push(index_u32, -cost);
    // ...
}

fn heap_pop(&mut self) -> Option<GenerationalIndex> {
    while let Some((index_u32, _)) = self.open.pop() {
        let stored_gen = self.heap_generations.get(index_u32)?;
        let idx = GenerationalIndex::new(index_u32 as u32, *stored_gen);
        if self.storage.is_valid(idx) {
            return Some(idx);
        }
    }
    None
}
```

**Pros**: Dense storage, O(1) lookup **Cons**: Need to resize vector, stores generation even when not in heap

**Recommendation**: **Option 1** (HashMap) for simplicity, **Option 3** for performance if heap is large.

### Slot Metadata Design

**Decision**: Store metadata in the slot data, not separate structure.

```rust
struct StateSlot<S, T> {
    state: S,
    cost_from_start: f64,
    parent_idx: Option<GenerationalIndex>,
    // ...
}

// Storage holds StateSlot, not just State
storage: GenerationalSlotStorage<StateSlot<S, T>>,
```

**Alternative**: Separate metadata storage indexed by `GenerationalIndex`.

**Rationale**:

- Simpler - all state data in one place
- No separate synchronization needed
- Cache-friendly (data and metadata together)

### Free List Strategy

**Decision**: Use LIFO stack (Vec) for free list.

**Alternatives**:

- FIFO queue (older slots reused first)
- Priority queue (reuse most fragmented slots)

**Rationale**: LIFO is simplest and cache-friendly (reuses recently freed slots).

## Performance Characteristics

### Memory

- **Per slot**: `generation: u32` (4 bytes) + `Option<T>` (1 byte tag + T size)
- **Free list**: `Vec<u32>` - O(free_count) space
- **Heap mapping**: `HashMap<usize, GenerationalIndex>` - O(heap_size) space

**Comparison to current**:

- Current: `ref_count: u32` (4 bytes) + `free_indices: Vec<usize>`
- New: `generation: u32` (4 bytes) + `free_list: Vec<u32>` + `heap_indices: HashMap`
- **Trade-off**: Extra HashMap for heap, but eliminates ref counting complexity

### Time Complexity

| Operation       | Current              | With Generational                  | Notes                    |
| --------------- | -------------------- | ---------------------------------- | ------------------------ |
| Allocate        | O(1)                 | O(1)                               | Both amortized           |
| Free            | O(1)                 | O(1)                               | Both amortized           |
| Get             | O(1)                 | O(1)                               | Same                     |
| Heap Push       | O(1) + ref count     | O(1) + hash insert                 | Extra HashMap lookup     |
| Heap Pop        | O(log n) + ref count | O(log n) + hash lookup + gen check | Generation check is O(1) |
| Stale Detection | Manual ref count     | Automatic (gen check)              | Zero-cost abstraction    |

### Cache Performance

- **Dense storage**: Same as current - slots stored in Vec
- **Free list**: LIFO helps reuse recently freed slots (better cache locality)
- **Heap mapping**: HashMap has cache overhead, but heap size is typically small

## Migration Path

### Phase 1: Implement GenerationalSlotStorage (Separate Module)

1. Create `src/state_pool/generational_slots.rs`
2. Implement `GenerationalIndex`, `Slot<T>`, `GenerationalSlotStorage`
3. Add unit tests

### Phase 2: Integrate with StatePool (Feature Flag)

1. Add feature flag `generational-slots` (disabled by default)
2. Make `StatePool` generic over storage type (or use type alias)
3. Test both implementations side-by-side

### Phase 3: Switch Default

1. Enable `generational-slots` by default
2. Remove old ref counting code
3. Update tests

### Phase 4: Remove Old Code

1. Remove feature flag
2. Remove old implementation
3. Clean up code

## Testing Strategy

### Unit Tests for GenerationalSlotStorage

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocate_new_slot() {
        let mut storage = GenerationalSlotStorage::new(8);
        let idx = storage.allocate(42);
        assert_eq!(idx.index(), 0);
        assert_eq!(idx.generation(), 0);
        assert_eq!(storage.get(idx), Some(&42));
    }

    #[test]
    fn free_and_reuse_increments_generation() {
        let mut storage = GenerationalSlotStorage::new(8);
        let idx1 = storage.allocate(10);
        storage.free(idx1);
        let idx2 = storage.allocate(20);

        assert_eq!(idx1.index(), idx2.index());  // Same slot
        assert_eq!(idx2.generation(), 1);  // Generation incremented
        assert_eq!(storage.get(idx1), None);  // Old index is stale
        assert_eq!(storage.get(idx2), Some(&20));  // New index works
    }

    #[test]
    fn stale_index_returns_none() {
        let mut storage = GenerationalSlotStorage::new(8);
        let idx1 = storage.allocate(10);
        storage.free(idx1);
        let idx2 = storage.allocate(20);

        // Old index should be invalid
        assert_eq!(storage.get(idx1), None);
        assert_eq!(storage.free(idx1), None);  // Can't free stale index
    }

    #[test]
    fn capacity_grows_correctly() {
        let mut storage = GenerationalSlotStorage::new(2);
        let idx1 = storage.allocate(1);
        let idx2 = storage.allocate(2);
        let idx3 = storage.allocate(3);  // Should grow

        assert_eq!(storage.capacity(), 3);
        assert_eq!(idx3.index(), 2);
    }
}
```

### Integration Tests with Heap

```rust
#[test]
fn heap_with_generational_indices() {
    let mut pool = StatePool::new(8);
    let idx = pool.enqueue_or_update_state(/* ... */);
    pool.heap_push(idx, 10.0);

    // Free the state
    pool.storage.free(idx);

    // Pop from heap - should skip stale entry
    let popped = pool.heap_pop();
    assert!(popped.is_none() || popped.unwrap() != idx);
}
```

## API Comparison

### Current API

```rust
// Manual ref counting
pool.increment_ref_count(idx);
pool.heap_push(&handle, cost);
pool.decrement_ref_count(idx);  // Manual cleanup

// Manual free list management
pool.free_indices.push(idx);
let idx = pool.free_indices.pop();
```

### Proposed API

```rust
// Automatic generation management
let idx = storage.allocate(data);
storage.free(idx);  // Automatically increments generation

// Heap automatically skips stale entries
pool.heap_push(idx, cost);  // No ref counting!
let popped = pool.heap_pop();  // Automatically validates generation
```

**Key Improvement**: No manual ref counting for heap membership - generation check handles it automatically.

## Zero-Cost Aspects

1. **Generation Check**: Single `cmp` instruction (same as ref count check)
2. **Index Extraction**: Bit mask (compiles to single instruction)
3. **Packing**: Bit shifts (compile-time optimized)
4. **Validation**: Single comparison (`slot.generation == idx.generation`)

**Zero overhead compared to ref counting**: Same memory (u32 generation vs u32 ref_count), same checks (comparison),
simpler logic.

## Open Questions

1. **Heap Mapping Strategy**: HashMap vs Vec for tracking heap entries?
   - HashMap: O(1) lookup, O(heap_size) space
   - Vec: O(1) lookup, O(capacity) space, but need to track which are in heap

2. **Free List Size**: Should we limit free list size to avoid memory bloat?
   - Or always maintain it for fast reuse?

3. **Parent Relationships**: Can we use generational indices for parents too?
   - If parent is always in heap, can skip ref counting?
   - Or if parent lifetime is independent, still need ref counting?

4. **state_to_idx Cleanup**: When to remove stale mappings?
   - On free (like current)?
   - On generation mismatch during lookup?

5. **Metadata Location**: Embed in slot data vs separate storage?
   - Current design embeds (simpler)
   - Alternative: separate metadata storage indexed by GenerationalIndex
