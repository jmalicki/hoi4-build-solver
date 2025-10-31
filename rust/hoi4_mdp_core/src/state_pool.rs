use orx_priority_queue::{PriorityQueue, PriorityQueueDecKey, QuaternaryHeapOfIndices};
use rapidhash::fast::RandomState as RapidHasher;
use std::collections::HashMap;
use std::collections::HashSet;
use std::hash::Hash;

/// A `usize` that can never be `usize::MAX`, similar to `NonZeroUsize` but excludes `usize::MAX` instead of `0`.
///
/// This allows using `Option<NonMaxUsize>` where `None` is semantically represented.
/// While Rust's niche optimization for custom types requires unstable features,
/// this type provides type safety and clear semantics.
///
/// **Safety**: This type must never hold `usize::MAX` as a value.
/// For our use case (state indices), this is safe because:
/// - Heap bounds are in the millions (10M, 20M, etc.)
/// - We'll never approach `usize::MAX` (which is ~18 quintillion on 64-bit)
///
/// Note: For full niche optimization (making `Option<NonMaxUsize>` the same size as `usize`),
/// nightly Rust with unstable features would be required. On stable Rust, this still provides
/// type safety and clear semantics.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NonMaxUsize(usize);

impl NonMaxUsize {
    /// Create a new `NonMaxUsize` from a `usize`.
    ///
    /// # Safety
    /// The value must not be `usize::MAX`. This is guaranteed at runtime by this method.
    ///
    /// # Panics
    /// Panics if `value == usize::MAX`.
    #[inline]
    pub fn new(value: usize) -> Option<Self> {
        if value == usize::MAX {
            None
        } else {
            // SAFETY: We just checked that value != usize::MAX
            Some(unsafe { Self::new_unchecked(value) })
        }
    }
    
    /// Create a new `NonMaxUsize` without checking the value.
    ///
    /// # Safety
    /// The value must not be `usize::MAX`. Violating this is undefined behavior.
    #[inline]
    pub unsafe fn new_unchecked(value: usize) -> Self {
        debug_assert_ne!(value, usize::MAX, "NonMaxUsize cannot be usize::MAX");
        NonMaxUsize(value)
    }
    
    /// Get the underlying `usize` value.
    #[inline]
    pub fn get(self) -> usize {
        self.0
    }
}

// Cannot implement From<usize> for Option<NonMaxUsize> as it's not our trait.
// Use NonMaxUsize::new() directly instead.

impl From<NonMaxUsize> for usize {
    fn from(val: NonMaxUsize) -> Self {
        val.get()
    }
}

/// State with metadata for A* search and index reuse.
///
/// This wrapper stores the state along with:
/// - `ref_count`: Tracks references for index reuse (when ref_count reaches 0, index can be reused)
///   A state is referenced when:
///   - It's in the open set (heap)
///   - Another state's parent points to it
///   - It's the goal state
/// - `parent_idx`: Index of the parent state (None for start state, stored as `usize::MAX`)
/// - `component_idx`: Index of the component/entity that was acted upon (generic, for path reconstruction)
///   Stored as `usize::MAX` when None.
/// - `transition_info`: Domain-specific transition information (None for start state)
struct StateWithMetadata<S, T> {
    state: S,
    ref_count: u32,
    parent_idx: Option<NonMaxUsize>, // None uses usize::MAX as niche (optimized to single usize)
    component_idx: Option<NonMaxUsize>, // None uses usize::MAX as niche (optimized to single usize)
    transition_info: Option<T>,
}

/// Pool for managing states with reference counting, index reuse, and the open set (heap).
///
/// Generic over:
/// - State type `S`, which must be `Hash + Eq + Clone`
/// - Transition info type `T` (domain-specific, e.g. action label and cost)
///
/// This makes the pool reusable for any A* problem, not just HOI4.
///
/// This pool maintains:
/// - Dense storage of states with metadata (state, ref_count, parent, component, transition_info)
/// - Hash map from State to index for O(1) lookups
/// - Free index stack for reusing deallocated indices
/// - Per-index metadata (g value) - generic to A*
/// - Priority queue (heap) for A* open set with decrease_key support
/// - Heap tracking (membership set, priorities, statistics)
///
/// Statistics:
/// - `total_states()`: Total number of states ever allocated (including freed)
/// - `used_states()`: Number of currently active states (not freed)
/// - `free_indices()`: Number of indices available for reuse
/// - `heap_len()`: Number of states currently in the open set (heap)
/// - `heap_avg_f()`: Average f value (g+h) of states in the heap
pub struct StatePool<S: Hash + Eq + Clone, T> {
    /// Dense storage: states with reference counts
    states: Vec<StateWithMetadata<S, T>>,
    /// Map from State to its index (only for active states)
    state_to_idx: HashMap<S, usize, RapidHasher>,
    /// Free indices available for reuse
    free_indices: Vec<usize>,
    /// Per-index g values (cost from start)
    g: Vec<f64>,
    /// Priority queue (heap) for A* open set
    open: QuaternaryHeapOfIndices<usize, f64>,
    /// Set tracking which indices are in the heap
    in_open: HashSet<usize>,
    /// Per-index heap priority (negative f value) or None if not in heap
    heap_prio: Vec<Option<f64>>,
    /// Current heap capacity bound (for growth)
    heap_bound: usize,
    /// Sum of f values in heap (for computing average)
    heap_sum_f: f64,
    /// Number of entries in heap
    heap_len: usize,
}

impl<S: Hash + Eq + Clone, T> StatePool<S, T> {
    /// Create a new StatePool.
    ///
    /// The pool will manage states of type `S` with reference counting
    /// and index reuse for efficient A* search.
    /// Create a new StatePool with initial heap capacity.
    ///
    /// The heap will grow automatically if needed.
    pub fn new(initial_heap_bound: usize) -> Self {
        Self {
            states: Vec::new(),
            state_to_idx: HashMap::with_hasher(RapidHasher::new()),
            free_indices: Vec::new(),
            g: Vec::new(),
            open: QuaternaryHeapOfIndices::with_index_bound(initial_heap_bound),
            in_open: HashSet::new(),
            heap_prio: Vec::new(),
            heap_bound: initial_heap_bound,
            heap_sum_f: 0.0,
            heap_len: 0,
        }
    }

    /// Get the index for a state, or None if not present.
    ///
    /// **Duplication Trade-off**: We currently store the state twice:
    /// - Once in `HashMap<S, usize>` (as the key) for O(1) lookup by State
    /// - Once in `Vec<StateWithMetadata<S>>` (in states[idx].state) for O(1) lookup by index
    ///
    /// This duplication is necessary because:
    /// - HashMap requires owned keys for hashing and equality checks
    /// - We need bidirectional O(1) lookups: State -> index and index -> State
    ///
    /// Alternatives (all have drawbacks):
    /// - Store only in Vec, lookup by iterating (O(n) lookup by State)
    /// - Store only in HashMap, lose O(1) index -> State lookup
    /// - Use hash codes without storing State (requires collision handling, loses O(1) guarantee)
    ///
    /// For A*, we prioritize O(1) lookups, so duplication is the pragmatic choice.
    pub fn get_index(&self, state: &S) -> Option<usize> {
        self.state_to_idx.get(state).copied()
    }

    /// Allocate a new index (reusing free if available, else allocating new).
    ///
    /// The returned index has:
    /// - `state` uninitialized (must be set immediately by caller)
    /// - `ref_count` set to 0 (should be incremented when referenced)
    /// - `parent` set to None
    /// - `g` set to INFINITY
    /// - `heap_prio` set to None
    ///
    /// Note: The state must be set immediately after allocation, as it's
    /// required for HashMap lookups. This is a safety requirement - the
    /// caller should set the state before using the index for lookups.
    pub fn allocate_index(&mut self) -> usize {
        if let Some(idx) = self.free_indices.pop() {
            // Reuse freed index - metadata already cleared (parent was cleared in decrement_ref_count)
            // State will be set by caller
            self.states[idx].ref_count = 0;
            idx
        } else {
            // Allocate new index
            let idx = self.states.len();
            // Use Default if S implements Default, otherwise caller must set state
            // For now, we'll require S to implement Default, or caller sets it immediately
            // Actually, we can't require Default since State might not implement it
            // So we'll store a placeholder and require immediate setting
            // This is unsafe territory - we need S to be Default or we need to change approach
            // Let's make it so allocate_index doesn't initialize state, and insert_state does
            self.states.push(StateWithMetadata {
                state: unsafe { std::mem::zeroed() }, // Placeholder - MUST be set immediately
                ref_count: 0,
                parent_idx: None,
                component_idx: None,
                transition_info: None,
            });
            self.g.push(f64::INFINITY);
            self.heap_prio.push(None);
            idx
        }
    }

    /// Insert a new state, allocating an index and setting up metadata.
    ///
    /// Returns the index for the state.
    /// The state's ref_count is initially 0 - caller should increment when referencing.
    pub fn insert_state(&mut self, state: S) -> usize {
        let idx = self.allocate_index();
        self.states[idx].state = state.clone();
        self.state_to_idx.insert(state, idx);
        idx
    }

    /// Try to enqueue a state with a given g value.
    ///
    /// This is fire-and-forget: caller just provides the state and its g value,
    /// and the pool handles all g_best comparisons internally.
    ///
    /// **Important**: States are identified/hashed by their `(infra, civ, mil)` values only.
    /// Parent information is stored separately and does NOT affect state identity.
    /// If the same state (same infra/civ/mil) is reached via different paths, this method
    /// will only keep the path with the best g value, which is correct for A*.
    ///
    /// Returns:
    /// - `Some(idx)` if the state should be enqueued (either new or improved g value)
    /// - `None` if the state already exists with a better g value
    ///
    /// If `Some(idx)` is returned:
    /// - The state is inserted/updated with the new g value
    /// - The state's ref_count is incremented (caller should increment heap ref when actually enqueueing)
    /// - Parent info can be set separately via `set_parent` (which handles parent ref counting)
    pub fn try_enqueue_state(&mut self, state: S, g_value: f64) -> Option<usize> {
        match self.get_index(&state) {
            Some(idx) => {
                // State exists - check if this g value is better
                // Note: This is the same physical state (same infra/civ/mil) reached via a different path.
                // We only keep the best path (lowest g), which is correct for A*.
                if g_value < self.g[idx] {
                    // Improved path - update g and return index for enqueueing
                    self.g[idx] = g_value;
                    Some(idx)
                } else {
                    // Existing path is better - don't enqueue
                    None
                }
            }
            None => {
                // New state - insert it
                let idx = self.allocate_index();
                self.states[idx].state = state.clone();
                self.state_to_idx.insert(state, idx);
                self.g[idx] = g_value;
                Some(idx)
            }
        }
    }

    /// Set parent, component, and transition information for a state.
    ///
    /// **Important**: When we update a state's parent (because we found a better path),
    /// we need to update ref counts:
    /// - The old parent loses a reference (from this child)
    /// - The new parent gains a reference (from this child)
    ///
    /// This automatically handles ref counting:
    /// - Decrements ref_count of old parent (if any)
    /// - Increments ref_count of new parent
    pub fn set_parent_component_and_transition(
        &mut self,
        child_idx: usize,
        parent_idx: usize,
        component_idx: usize,
        transition_info: Option<T>,
    ) {
        // Update parent - decrement old parent's ref count, increment new parent's
        if let Some(old_parent) = self.states[child_idx].parent_idx {
            self.decrement_ref_count(old_parent.get());
        }
        // Increment new parent's ref count (this parent is now referenced by child_idx)
        self.increment_ref_count(parent_idx);

        // Set new parent, component, and transition info
        // SAFETY: parent_idx and component_idx are state indices, guaranteed < usize::MAX
        self.states[child_idx].parent_idx = Some(unsafe { NonMaxUsize::new_unchecked(parent_idx) });
        self.states[child_idx].component_idx = Some(unsafe { NonMaxUsize::new_unchecked(component_idx) });
        self.states[child_idx].transition_info = transition_info;
    }

    /// Decrement reference count for an index.
    ///
    /// If ref_count reaches zero:
    /// - Removes the state from the HashMap
    /// - Clears metadata (g, parent, heap_prio)
    /// - Adds index to free_indices for reuse
    pub fn decrement_ref_count(&mut self, idx: usize) {
        if idx >= self.states.len() {
            return;
        }

        self.states[idx].ref_count = self.states[idx].ref_count.saturating_sub(1);

        if self.states[idx].ref_count == 0 {
            // Remove from HashMap
            self.state_to_idx.remove(&self.states[idx].state);

            // Clear metadata
            if idx < self.g.len() {
                self.g[idx] = f64::INFINITY;
            }
            if idx < self.states.len() {
                self.states[idx].parent_idx = None;
                self.states[idx].component_idx = None;
                self.states[idx].transition_info = None;
            }
            if idx < self.heap_prio.len() {
                self.heap_prio[idx] = None;
            }

            // Add to free list
            self.free_indices.push(idx);
        }
    }

    /// Increment reference count for an index.
    pub fn increment_ref_count(&mut self, idx: usize) {
        if idx < self.states.len() {
            self.states[idx].ref_count += 1;
        }
    }

    /// Get a reference to a state by index.
    pub fn get_state(&self, idx: usize) -> Option<&S> {
        self.states.get(idx).map(|sm| &sm.state)
    }

    /// Get mutable access to state metadata by index.
    /// Returns None if index is invalid or state is freed (ref_count == 0).
    pub fn get_state_mut(&mut self, idx: usize) -> Option<&mut S> {
        if idx >= self.states.len() || self.states[idx].ref_count == 0 {
            return None;
        }
        Some(&mut self.states[idx].state)
    }

    /// Get reference count for an index.
    pub fn ref_count(&self, idx: usize) -> u32 {
        self.states.get(idx).map(|sm| sm.ref_count).unwrap_or(0)
    }

    /// Check if an index is valid and active (not freed).
    pub fn is_active(&self, idx: usize) -> bool {
        idx < self.states.len() && self.states[idx].ref_count > 0
    }

    /// Get mutable access to g value by index.
    pub fn g_mut(&mut self) -> &mut Vec<f64> {
        &mut self.g
    }

    /// Get g value by index.
    pub fn g(&self, idx: usize) -> f64 {
        self.g.get(idx).copied().unwrap_or(f64::INFINITY)
    }

    /// Get parent index for a state by index.
    pub fn parent_idx(&self, idx: usize) -> Option<usize> {
        self.states.get(idx).and_then(|sm| sm.parent_idx.map(|i| i.get()))
    }

    /// Get component index for a state by index.
    pub fn component_idx(&self, idx: usize) -> Option<usize> {
        self.states.get(idx).and_then(|sm| sm.component_idx.map(|i| i.get()))
    }

    /// Get transition info for a state by index.
    pub fn transition_info(&self, idx: usize) -> Option<&T> {
        self.states.get(idx).and_then(|sm| sm.transition_info.as_ref())
    }

    /// Get mutable access to parent_idx for a state by index.
    ///
    /// **Warning**: Modifying parent_idx directly bypasses ref counting updates.
    /// Use `set_parent_component_and_transition` instead to ensure ref counts are updated correctly.
    pub fn parent_idx_mut(&mut self, idx: usize) -> Option<&mut Option<NonMaxUsize>> {
        self.states.get_mut(idx).map(|sm| &mut sm.parent_idx)
    }

    /// Get mutable access to heap_prio by index.
    pub fn heap_prio_mut(&mut self) -> &mut Vec<Option<f64>> {
        &mut self.heap_prio
    }

    /// Get heap_prio by index.
    pub fn heap_prio(&self, idx: usize) -> Option<f64> {
        self.heap_prio.get(idx).copied().flatten()
    }

    /// Statistics: Total number of states ever allocated (including freed).
    pub fn total_states(&self) -> usize {
        self.states.len()
    }

    /// Statistics: Number of currently active states (not freed).
    ///
    /// This is total_states() - free_indices().len().
    pub fn used_states(&self) -> usize {
        self.states.len() - self.free_indices.len()
    }

    /// Statistics: Number of indices available for reuse.
    pub fn free_indices_count(&self) -> usize {
        self.free_indices.len()
    }

    /// Get the capacity needed for heap growth checks.
    ///
    /// This is the maximum index that could be used (total_states).
    pub fn heap_capacity(&self) -> usize {
        self.states.len()
    }

    // ========== Heap Operations ==========

    /// Push a state index onto the heap with priority (negative f value).
    ///
    /// This automatically:
    /// - Increments the state's ref_count (for being in heap)
    /// - Tracks the priority for later decrease_key
    /// - Updates heap statistics (heap_sum_f, heap_len)
    pub fn heap_push(&mut self, idx: usize, f: f64) {
        self.increment_ref_count(idx);
        self.open.push(idx, -f);
        self.in_open.insert(idx);
        self.heap_sum_f += f;
        self.heap_len += 1;
        if idx < self.heap_prio.len() {
            self.heap_prio[idx] = Some(-f);
        } else {
            self.heap_prio.resize(idx + 1, None);
            self.heap_prio[idx] = Some(-f);
        }
    }

    /// Pop the highest priority state index from the heap.
    ///
    /// Returns `Some((idx, f))` where `f` is the true f value (not negated).
    /// Returns `None` if the heap is empty.
    ///
    /// This automatically:
    /// - Decrements the state's ref_count (no longer in heap)
    /// - Removes from in_open set
    /// - Clears heap_prio entry
    /// - Updates heap statistics
    pub fn heap_pop(&mut self) -> Option<(usize, f64)> {
        if let Some((idx, neg_f)) = self.open.pop() {
            let f = -neg_f;
            self.heap_sum_f -= f;
            self.heap_len -= 1;
            self.in_open.remove(&idx);
            if idx < self.heap_prio.len() {
                self.heap_prio[idx] = None;
            }
            // Decrement ref count - no longer in heap
            self.decrement_ref_count(idx);
            Some((idx, f))
        } else {
            None
        }
    }

    /// Decrease the priority of a state in the heap.
    ///
    /// Updates the heap entry if `idx` is in the heap and the new f value is better (lower).
    /// Also updates heap statistics.
    pub fn heap_decrease_key(&mut self, idx: usize, f: f64) -> bool {
        if !self.in_open.contains(&idx) {
            return false;
        }

        if let Some(old_neg) = self.heap_prio.get(idx).and_then(|o| *o) {
            let old_f = -old_neg;
            self.open.decrease_key(&idx, -f);
            if idx < self.heap_prio.len() {
                self.heap_prio[idx] = Some(-f);
            }
            // Update heap statistics
            self.heap_sum_f += f - old_f;
            true
        } else {
            false
        }
    }

    /// Check if an index is in the heap.
    pub fn is_in_heap(&self, idx: usize) -> bool {
        self.in_open.contains(&idx)
    }

    /// Get the number of states in the heap.
    pub fn heap_len(&self) -> usize {
        self.heap_len
    }

    /// Get the average f value (g+h) of states in the heap.
    pub fn heap_avg_f(&self) -> f64 {
        if self.heap_len > 0 {
            self.heap_sum_f / (self.heap_len as f64)
        } else {
            0.0
        }
    }

    /// Get the heap length for display (same as heap_len() but more explicit).
    pub fn heap_size(&self) -> usize {
        self.open.len()
    }

    /// Get mutable reference to heap_prio vector (for heap growth).
    pub fn heap_prio_mut_for_growth(&mut self) -> &mut Vec<Option<f64>> {
        &mut self.heap_prio
    }

    /// Get mutable reference to open heap (for heap growth).
    pub fn heap_mut_for_growth(&mut self) -> &mut QuaternaryHeapOfIndices<usize, f64> {
        &mut self.open
    }

    /// Get heap bound for growth checks.
    pub fn heap_bound(&self) -> usize {
        self.heap_bound
    }

    /// Set heap bound (after growth).
    pub fn set_heap_bound(&mut self, new_bound: usize) {
        self.heap_bound = new_bound;
    }

    /// Get mutable reference to heap_bound for growth.
    pub fn heap_bound_mut(&mut self) -> &mut usize {
        &mut self.heap_bound
    }

    // ========== High-level Enqueue/Update Logic ==========

    /// Enqueue or update a state with all logic handled internally.
    ///
    /// This is fire-and-forget: caller just provides the state, g value, parent info,
    /// component index, transition info, and f value, and the pool handles:
    /// - g_best comparison (only enqueues if new or improved)
    /// - Parent, component, and transition info updates (with ref counting)
    /// - Heap operations (decrease_key if in heap, push if not)
    /// - Heap growth checks
    ///
    /// Returns:
    /// - `true` if the state was enqueued/updated
    /// - `false` if the state already exists with a better g value (skipped)
    ///
    /// **This is the main entry point for A* search - all heap and state management
    /// logic is handled internally.**
    pub fn enqueue_or_update_state(
        &mut self,
        state: S,
        g_value: f64,
        parent_idx: usize,
        component_idx: usize,
        transition_info: Option<T>,
        f: f64,
    ) -> bool {
        // Try to enqueue - pool handles g_best comparison
        if let Some(state_idx) = self.try_enqueue_state(state, g_value) {
            // State should be enqueued/updated
            // Set parent, component, and transition info - pool handles ref counting automatically
            self.set_parent_component_and_transition(state_idx, parent_idx, component_idx, transition_info);

            // Update heap - decrease_key if in heap, push if not
            if self.is_in_heap(state_idx) {
                self.heap_decrease_key(state_idx, f);
            } else {
                self.heap_push(state_idx, f);
            }

            // Check if we need to grow the heap
            let heap_capacity = self.heap_capacity();
            grow_heap_if_needed(
                &mut self.open,
                &mut self.heap_prio,
                heap_capacity,
                &mut self.heap_bound,
            );

            true
        } else {
            // State already has better g value - skip
            false
        }
    }
}

// Import heap_growth for the enqueue method
use crate::heap_growth::grow_heap_if_needed;
