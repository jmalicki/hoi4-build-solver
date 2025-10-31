//! State pool: stable indexing, ref-counted ownership, and deferred reclamation.
//!
//! This module provides `StatePool`, a dense storage for search states with
//! stable indices, plus an indexed open set backed by a quaternary heap.
//!
//! Key behaviors:
//! - Reference counting: heap membership and live `StateHandle`s contribute to
//!   a state's `ref_count`. When it drops to zero, the state enters a
//!   zero-refcount queue but remains fully initialized and addressable.
//! - Deferred freeing: zero-ref states are not immediately deinitialized nor
//!   removed from `state_to_idx`; this preserves best-known g-cost for
//!   heuristics. Actual reclamation occurs only when capacity is needed.
//! - Heap growth: indices pushed to the heap must be < `heap_bound`. The pool
//!   grows the heap bound on demand via `grow_heap_if_needed`.
//!
//! See also `README.md` in this directory for diagrams and lifecycle flows.
use std::collections::{HashMap, HashSet, VecDeque};
use std::hash::Hash;
use orx_priority_queue::{PriorityQueue, PriorityQueueDecKey, QuaternaryHeapOfIndices};
use rapidhash::fast::RandomState as RapidHasher;

use crate::heap_growth::grow_heap_if_needed;
use super::{NonMaxUsize, StateHandle};

/// Per-index record stored by the pool.
///
/// Contains the state payload plus search metadata needed for heuristics and
/// path reconstruction. States remain initialized even after `ref_count`
/// reaches zero so that best-known costs are available until reclamation.
struct StateWithMetadata<S, T> {
    /// The state payload. Used as the key in `state_to_idx` for identity.
    state: S,
    /// Number of owners of this state (heap membership + live handles).
    ref_count: u32,
    /// Best-known g-cost (cost from start). Preserved while in zero-ref queue
    /// so heuristics can still consult it.
    cost_from_start: f64,
    /// Optional index of the parent state for path reconstruction.
    parent_idx: Option<NonMaxUsize>,
    /// Optional component grouping (domain-specific clustering/indexing).
    component_idx: Option<NonMaxUsize>,
    /// Optional transition/action info used to reach this state.
    transition_info: Option<T>,
}

impl<S: Default, T> Default for StateWithMetadata<S, T> {
    fn default() -> Self {
        Self {
            state: S::default(),
            ref_count: 0,
            cost_from_start: f64::INFINITY,
            parent_idx: None,
            component_idx: None,
            transition_info: None,
        }
    }
}

/// Dense pool that owns all states and manages their lifecycle.
///
/// Responsibilities:
/// - Assign stable indices for states and maintain `state_to_idx` mapping.
/// - Track ownership through `ref_count` and defer freeing via
///   `zero_ref_queue`.
/// - Manage the `open` priority queue and ensure `heap_bound` safety.
pub struct StatePool<S: Hash + Eq + Clone + Default, T> {
    /// Dense storage of states and metadata by stable index.
    states: Vec<StateWithMetadata<S, T>>,
    /// Map from state payload to its stable index.
    state_to_idx: HashMap<S, usize, RapidHasher>,
    /// FIFO queue of indices with `ref_count == 0`. Entries remain initialized
    /// and mapped until actually reclaimed for reuse.
    zero_ref_queue: VecDeque<usize>,
    /// Open set: heap of indices keyed by `-f` (lower is better).
    open: QuaternaryHeapOfIndices<usize, f64>,
    /// Membership set for O(1) checks and safe decrease-key.
    in_open: HashSet<usize>,
    /// Current maximum supported index for `open`. Grows on demand.
    heap_bound: usize,
    /// Sum of finite f-values currently in `open` (for average diagnostics).
    heap_sum_f: f64,
    /// Number of entries in `open` (for diagnostics and averages).
    heap_len: usize,
}

impl<S: Hash + Eq + Clone + Default, T> StatePool<S, T> {
    /// Create a new pool with an initial heap index bound.
    ///
    /// The heap bound grows automatically as needed when inserting higher
    /// indices.
    pub fn new(initial_heap_bound: usize) -> Self {
        Self {
            states: Vec::new(),
            state_to_idx: HashMap::with_hasher(RapidHasher::new()),
            zero_ref_queue: VecDeque::new(),
            open: QuaternaryHeapOfIndices::with_index_bound(initial_heap_bound),
            in_open: HashSet::new(),
            heap_bound: initial_heap_bound,
            heap_sum_f: 0.0,
            heap_len: 0,
        }
    }

    /// Look up the stable index for a state payload.
    fn get_index(&self, state: &S) -> Option<usize> { self.state_to_idx.get(state).copied() }

    /// Obtain an index slot for a new or updated state.
    ///
    /// Prefers reclaiming from `zero_ref_queue` (performing actual freeing by
    /// removing the old mapping and resetting metadata). If the queue is empty,
    /// appends a new slot.
    fn allocate_index(&mut self) -> usize {
        if let Some(idx) = self.zero_ref_queue.pop_front() {
            // We are now actually reclaiming this slot for reuse.
            self.state_to_idx.remove(&self.states[idx].state);
            let _ = std::mem::take(&mut self.states[idx]);
            idx
        } else {
            let idx = self.states.len();
            self.states.push(StateWithMetadata::default());
            idx
        }
    }

    /// Insert a state payload into the pool, returning its stable index.
    ///
    /// May reclaim a zero-ref slot or append a new one.
    pub fn insert_state(&mut self, state: S) -> usize {
        let idx = self.allocate_index();
        self.states[idx].state = state.clone();
        self.state_to_idx.insert(state, idx);
        idx
    }

    /// Insert-or-update helper for best-known g-cost.
    ///
    /// - If the state exists and `cost_value` improves `cost_from_start`, it is
    ///   updated and the index is returned.
    /// - If the state does not exist, it is inserted with the given cost.
    /// - Otherwise returns `None` when no improvement occurs.
    fn try_update_best_cost(&mut self, state: S, cost_value: f64) -> Option<NonMaxUsize> {
        match self.get_index(&state) {
            Some(idx) => {
                if cost_value < self.states[idx].cost_from_start {
                    self.states[idx].cost_from_start = cost_value;
                    Some(unsafe { NonMaxUsize::new_unchecked(idx) })
                } else {
                    None
                }
            }
            None => {
                let idx = self.allocate_index();
                self.states[idx].state = state.clone();
                self.state_to_idx.insert(state, idx);
                self.states[idx].cost_from_start = cost_value;
                Some(unsafe { NonMaxUsize::new_unchecked(idx) })
            }
        }
    }

    /// Update parent/component/transition metadata and maintain ref-counts.
    ///
    /// Decrements the previous parent's refcount (if any), increments the new
    /// parent's refcount (if any), and sets associated metadata on the child.
    fn set_parent_component_and_transition(&mut self, child_idx: usize, parent_idx: Option<usize>, component_idx: usize, transition_info: Option<T>) {
        // Decrement old parent ref count if present
        if let Some(old_parent) = self.states[child_idx].parent_idx { self.decrement_ref_count(old_parent.get()); }
        // Increment new parent ref count if present (start state has no parent)
        if let Some(pidx) = parent_idx {
            self.increment_ref_count(pidx);
            self.states[child_idx].parent_idx = Some(unsafe { NonMaxUsize::new_unchecked(pidx) });
        } else {
            self.states[child_idx].parent_idx = None;
        }
        self.states[child_idx].component_idx = Some(unsafe { NonMaxUsize::new_unchecked(component_idx) });
        self.states[child_idx].transition_info = transition_info;
    }

    /// Decrement refcount and enqueue the index when it reaches zero.
    ///
    /// Defers actual freeing (mapping removal and metadata reset) to the point
    /// where a new allocation needs to reclaim a slot.
    pub fn decrement_ref_count(&mut self, idx: usize) {
        if idx >= self.states.len() { return; }
        self.states[idx].ref_count = self.states[idx].ref_count.saturating_sub(1);
        if self.states[idx].ref_count == 0 {
            // Defer actual freeing: keep mapping and cost so heuristics can still consult it.
            // Place into zero-ref queue for potential reuse under capacity pressure.
            self.zero_ref_queue.push_back(idx);
        }
    }

    /// Increment a state's refcount if the index is valid.
    pub fn increment_ref_count(&mut self, idx: usize) { if idx < self.states.len() { self.states[idx].ref_count += 1; } }
    /// Access a state's payload by index.
    pub fn get_state(&self, idx: usize) -> Option<&S> { self.states.get(idx).map(|sm| &sm.state) }
    /// Best-known g-cost for an index (INFINITY if not present).
    pub fn cost_from_start(&self, idx: usize) -> f64 { self.states.get(idx).map(|sm| sm.cost_from_start).unwrap_or(f64::INFINITY) }
    /// Parent index if present.
    pub fn parent_idx(&self, idx: usize) -> Option<usize> { self.states.get(idx).and_then(|sm| sm.parent_idx.map(|i| i.get())) }
    /// Component index if present.
    pub fn component_idx(&self, idx: usize) -> Option<usize> { self.states.get(idx).and_then(|sm| sm.component_idx.map(|i| i.get())) }
    /// Transition info if present.
    pub fn transition_info(&self, idx: usize) -> Option<&T> { self.states.get(idx).and_then(|sm| sm.transition_info.as_ref()) }

    /// Total slots ever allocated (active + zero-ref + future appendable count).
    pub fn total_states(&self) -> usize { self.states.len() }
    /// Approximate number of in-use slots (excludes queued zero-ref entries).
    pub fn used_states(&self) -> usize { self.states.len() - self.zero_ref_queue.len() }
    /// Number of indices currently queued for potential reuse.
    pub fn free_indices_count(&self) -> usize { self.zero_ref_queue.len() }
    /// Current capacity suggestible to the heap (equal to `states.len()`).
    pub fn heap_capacity(&self) -> usize { self.states.len() }

    /// Push a handle's index into the open set with priority `f`.
    ///
    /// Ensures heap bound growth before insertion and updates accounting
    /// (`in_open`, `heap_sum_f`, `heap_len`). Increments refcount for heap
    /// ownership.
    pub fn heap_push(&mut self, handle: &StateHandle<S, T>, f: f64) {
        let idx = handle.index();
        // Guard: grow heap bound before inserting an index beyond current bound
        if idx >= self.heap_bound {
            while self.heap_bound <= idx {
                let _ = grow_heap_if_needed(&mut self.open, idx + 1, &mut self.heap_bound);
            }
            // Rebuild accounting based on current heap content
            self.in_open.clear();
            self.heap_sum_f = 0.0;
            self.heap_len = 0;
            let mut tmp: Vec<(usize, f64)> = Vec::with_capacity(self.open.len());
            while let Some((i, neg)) = self.open.pop() { tmp.push((i, neg)); }
            for (i, neg) in tmp.into_iter() {
                self.open.push(i, neg);
                self.in_open.insert(i);
                let f_i = -neg;
                if f_i.is_finite() { self.heap_sum_f += f_i; }
                self.heap_len += 1;
            }
        }
        self.increment_ref_count(idx);
        self.open.push(idx, -f);
        self.in_open.insert(idx);
        if f.is_finite() { self.heap_sum_f += f; }
        self.heap_len += 1;
    }
    /// Pop the best index from the open set and return a new `StateHandle`.
    ///
    /// Transfers ownership by creating a handle (increment) and dropping the
    /// heap's reference (decrement). Accounting is updated accordingly.
    pub fn heap_pop(&mut self) -> Option<StateHandle<S, T>> {
        if let Some((idx, neg_f)) = self.open.pop() {
            let f = -neg_f;
            // Transfer ownership: first create a handle (increments ref_count),
            // then drop the heap's reference (decrement).
            let handle = StateHandle::new(idx, f, self);
            if f.is_finite() { self.heap_sum_f -= f; }
            self.heap_len -= 1;
            self.in_open.remove(&idx);
            // Decrement heap membership reference; since the handle was just created,
            // ref_count stays > 0 and metadata (including g) is preserved.
            self.decrement_ref_count(idx);
            Some(handle)
        } else { None }
    }
    /// Decrease the priority of an index already present in the open set.
    fn heap_decrease_key(&mut self, idx: usize, f: f64) -> bool {
        if !self.in_open.contains(&idx) { return false; }
        self.open.decrease_key(&idx, -f);
        true
    }
    /// Whether an index is currently present in `open`.
    fn is_in_heap(&self, idx: usize) -> bool { self.in_open.contains(&idx) }
    /// Number of entries currently in `open` (accounted).
    pub fn heap_len(&self) -> usize { self.heap_len }
    /// Average of finite f-values in `open` (diagnostic only).
    pub fn heap_avg_f(&self) -> f64 { if self.heap_len == 0 { 0.0 } else { self.heap_sum_f / (self.heap_len as f64) } }
    /// Underlying heap length (may be used for sanity checks/testing).
    pub fn heap_size(&self) -> usize { self.open.len() }
    fn heap_mut_for_growth(&mut self) -> &mut QuaternaryHeapOfIndices<usize, f64> { &mut self.open }
    fn heap_bound(&self) -> usize { self.heap_bound }
    fn set_heap_bound(&mut self, new_bound: usize) { self.heap_bound = new_bound; }
    fn heap_bound_mut(&mut self) -> &mut usize { &mut self.heap_bound }

    /// Insert a state or update its best-known g, parent, and heap priority.
    ///
    /// Validates inputs, ensures heap bound growth when needed, and either
    /// decreases the key if present in `open` or pushes a new entry.
    pub fn enqueue_or_update_state(&mut self, state: S, cost_value: f64, parent: Option<&StateHandle<S, T>>, component_idx: usize, transition_info: Option<T>, f: f64) -> bool {
        if cost_value.is_nan() || f.is_nan() || cost_value < 0.0 || f < 0.0 { return false; }
        if let Some(state_idx_nm) = self.try_update_best_cost(state, cost_value) {
            let state_idx = state_idx_nm.get();
            let parent_idx = parent.map(|p| p.index());
            self.set_parent_component_and_transition(state_idx, parent_idx, component_idx, transition_info);
            // Ensure heap can accommodate this index BEFORE pushing/decreasing
            if state_idx >= self.heap_bound {
                while self.heap_bound <= state_idx {
                    let _ = grow_heap_if_needed(&mut self.open, state_idx + 1, &mut self.heap_bound);
                }
                // Rebuild accounting structures after growth
                self.in_open.clear();
                self.heap_sum_f = 0.0;
                self.heap_len = 0;
                let mut tmp: Vec<(usize, f64)> = Vec::with_capacity(self.open.len());
                while let Some((idx, neg_f)) = self.open.pop() { tmp.push((idx, neg_f)); }
                for (idx, neg_f) in tmp.into_iter() {
                    self.open.push(idx, neg_f);
                    self.in_open.insert(idx);
                    let f = -neg_f;
                    if f.is_finite() { self.heap_sum_f += f; }
                    self.heap_len += 1;
                }
            }
            if self.is_in_heap(state_idx) { self.heap_decrease_key(state_idx, f); } else {
                let handle = StateHandle::new(state_idx, f, self);
                self.heap_push(&handle, f);
            }
            let heap_capacity = self.heap_capacity();
            grow_heap_if_needed(&mut self.open, heap_capacity, &mut self.heap_bound);
            true
        } else { false }
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Hash, PartialEq, Eq, Debug, Default)]
    struct TestState(u32);

    #[derive(Clone, Debug, PartialEq)]
    struct TestTransition { action: &'static str, cost: f64 }

    fn make_pool() -> StatePool<TestState, TestTransition> {
        StatePool::new(1024)
    }

    // Test-only helpers to access internal state
    impl<S: Hash + Eq + Clone + Default, T> StatePool<S, T> {
        fn ref_count(&self, idx: usize) -> u32 { self.states.get(idx).map(|sm| sm.ref_count).unwrap_or(0) }
        fn is_active(&self, idx: usize) -> bool { idx < self.states.len() && self.states[idx].ref_count > 0 }
        fn make_handle(&mut self, idx: usize, cost: f64) -> StateHandle<S, T> { StateHandle::new(idx, cost, self) }
    }

    #[test]
    fn insert_and_lookup_preserves_identity() {
        // Requirement: Pool indexes states and allows retrieving them by index
        let mut pool = make_pool();
        let s = TestState(42);
        let idx = pool.insert_state(s.clone());
        assert_eq!(pool.get_state(idx), Some(&s));
        assert!(pool.is_active(idx) == false); // no refs yet
    }

    #[test]
    fn ref_count_increments_and_frees_on_zero() {
        // Requirement: States are freed and index reused when ref_count reaches zero
        let mut pool = make_pool();
        let idx = pool.insert_state(TestState(1));
        assert_eq!(pool.ref_count(idx), 0);
        pool.increment_ref_count(idx);
        assert_eq!(pool.ref_count(idx), 1);
        pool.decrement_ref_count(idx);
        assert_eq!(pool.ref_count(idx), 0);
        assert!(pool.free_indices_count() >= 1);
    }

    #[test]
    fn heap_push_pop_manages_refs() {
        // Requirement: Heap membership increments refs and pop decrements
        let mut pool = make_pool();
        let idx = pool.insert_state(TestState(2));
        let h = pool.make_handle(idx, 10.0);
        pool.heap_push(&h, 10.0);
        assert_eq!(pool.heap_size(), 1);
        // dropping the handle should leave one ref from heap
        drop(h);
        assert_eq!(pool.ref_count(idx), 1);
        let popped = pool.heap_pop();
        assert!(popped.is_some());
        // after pop, heap ref removed; handle holds a ref until drop
        let ph = popped.unwrap();
        assert_eq!(pool.heap_size(), 0);
        drop(ph);
        assert_eq!(pool.ref_count(idx), 0);
    }

    #[test]
    fn enqueue_or_update_rejects_negative_or_nan() {
        // Requirement: Enqueue must reject NaN or negative g/f
        let mut pool = make_pool();
        let parent_idx = pool.insert_state(TestState(10));
        let parent = pool.make_handle(parent_idx, 0.0);
        let ok = pool.enqueue_or_update_state(TestState(11), 1.0, Some(&parent), 0, Some(TestTransition{action:"a", cost:1.0}), 2.0);
        assert!(ok);
        let bad1 = pool.enqueue_or_update_state(TestState(12), f64::NAN, Some(&parent), 0, None, 2.0);
        let bad2 = pool.enqueue_or_update_state(TestState(13), 1.0, Some(&parent), 0, None, f64::NAN);
        let bad3 = pool.enqueue_or_update_state(TestState(14), -1.0, Some(&parent), 0, None, 2.0);
        let bad4 = pool.enqueue_or_update_state(TestState(15), 1.0, Some(&parent), 0, None, -2.0);
        assert!(!bad1 && !bad2 && !bad3 && !bad4);
    }

    #[test]
    fn enqueue_start_state_with_no_parent() {
        // Requirement: Start state can be enqueued with None parent
        let mut pool = make_pool();
        let ok = pool.enqueue_or_update_state(TestState(99), 0.0, None, 0, None, 10.0);
        assert!(ok);
        assert_eq!(pool.heap_size(), 1);
        let popped = pool.heap_pop();
        assert!(popped.is_some());
        let ph = popped.unwrap();
        assert_eq!(pool.parent_idx(ph.index()), None); // Start state has no parent
    }

    #[test]
    fn heap_push_grows_before_inserting_high_index() {
        // Requirement: pushing a handle with index >= bound must not panic; heap grows first
        let mut pool: StatePool<TestState, TestTransition> = StatePool::new(8);
        // Simulate many inserted states to create a high index handle
        let mut last_idx = 0usize;
        for i in 0..1024 {
            last_idx = pool.insert_state(TestState(i));
        }
        assert!(last_idx >= 1023);
        let h = pool.make_handle(last_idx, 1.0);
        // This would OOB if we didn't grow inside heap_push
        pool.heap_push(&h, 1.0);
        assert!(pool.heap_size() >= 1);
    }
}


