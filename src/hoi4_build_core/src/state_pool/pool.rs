use orx_priority_queue::{PriorityQueue, PriorityQueueDecKey, QuaternaryHeapOfIndices};
use rapidhash::fast::RandomState as RapidHasher;
use std::collections::{HashMap, HashSet};
use std::hash::Hash;

use super::{NonMaxUsize, StateHandle};
use crate::heap_growth::grow_heap_if_needed;

struct StateWithMetadata<S, T> {
    state: S,
    ref_count: u32,
    cost_from_start: f64,
    parent_idx: Option<NonMaxUsize>,
    component_idx: Option<NonMaxUsize>,
    transition_info: Option<T>,
}

pub struct StatePool<S: Hash + Eq + Clone + Default, T> {
    states: Vec<StateWithMetadata<S, T>>,
    state_to_idx: HashMap<S, usize, RapidHasher>,
    free_indices: Vec<usize>,
    open: QuaternaryHeapOfIndices<usize, f64>,
    in_open: HashSet<usize>,
    heap_bound: usize,
    heap_sum_estimated_total_cost: f64,
    heap_len: usize,
}

impl<S: Hash + Eq + Clone + Default, T> StatePool<S, T> {
    pub fn new(initial_heap_bound: usize) -> Self {
        Self {
            states: Vec::new(),
            state_to_idx: HashMap::with_hasher(RapidHasher::new()),
            free_indices: Vec::new(),
            open: QuaternaryHeapOfIndices::with_index_bound(initial_heap_bound),
            in_open: HashSet::new(),
            heap_bound: initial_heap_bound,
            heap_sum_estimated_total_cost: 0.0,
            heap_len: 0,
        }
    }

    fn get_index(&self, state: &S) -> Option<usize> {
        self.state_to_idx.get(state).copied()
    }

    fn allocate_index(&mut self) -> usize {
        if let Some(idx) = self.free_indices.pop() {
            self.states[idx].ref_count = 0;
            self.states[idx].cost_from_start = f64::INFINITY;
            idx
        } else {
            let idx = self.states.len();
            self.states.push(StateWithMetadata {
                state: S::default(),
                ref_count: 0,
                cost_from_start: f64::INFINITY,
                parent_idx: None,
                component_idx: None,
                transition_info: None,
            });
            idx
        }
    }

    #[allow(dead_code)]
    pub fn insert_state(&mut self, state: S) -> usize {
        let idx = self.allocate_index();
        self.states[idx].state = state.clone();
        self.state_to_idx.insert(state, idx);
        idx
    }

    fn try_update_best_cost(&mut self, state: S, path_cost: f64) -> Option<NonMaxUsize> {
        match self.get_index(&state) {
            Some(idx) => {
                if path_cost < self.states[idx].cost_from_start {
                    self.states[idx].cost_from_start = path_cost;
                    Some(unsafe { NonMaxUsize::new_unchecked(idx) })
                } else {
                    None
                }
            }
            None => {
                let idx = self.allocate_index();
                self.states[idx].state = state.clone();
                self.state_to_idx.insert(state, idx);
                self.states[idx].cost_from_start = path_cost;
                Some(unsafe { NonMaxUsize::new_unchecked(idx) })
            }
        }
    }

    fn set_parent_component_and_transition(
        &mut self,
        child_idx: usize,
        parent_idx: Option<usize>,
        component_idx: usize,
        transition_info: Option<T>,
    ) {
        // Decrement old parent ref count if present
        if let Some(old_parent) = self.states[child_idx].parent_idx {
            self.decrement_ref_count(old_parent.get());
        }
        // Increment new parent ref count if present (start state has no parent)
        if let Some(pidx) = parent_idx {
            self.increment_ref_count(pidx);
            self.states[child_idx].parent_idx = Some(unsafe { NonMaxUsize::new_unchecked(pidx) });
        } else {
            self.states[child_idx].parent_idx = None;
        }
        self.states[child_idx].component_idx =
            Some(unsafe { NonMaxUsize::new_unchecked(component_idx) });
        self.states[child_idx].transition_info = transition_info;
    }

    pub fn decrement_ref_count(&mut self, idx: usize) {
        if idx >= self.states.len() {
            return;
        }
        self.states[idx].ref_count = self.states[idx].ref_count.saturating_sub(1);
        if self.states[idx].ref_count == 0 {
            self.state_to_idx.remove(&self.states[idx].state);
            self.states[idx].cost_from_start = f64::INFINITY;
            self.states[idx].parent_idx = None;
            self.states[idx].component_idx = None;
            self.states[idx].transition_info = None;
            self.free_indices.push(idx);
        }
    }

    pub fn increment_ref_count(&mut self, idx: usize) {
        if idx < self.states.len() {
            self.states[idx].ref_count += 1;
        }
    }
    pub fn get_state(&self, idx: usize) -> Option<&S> {
        self.states.get(idx).map(|sm| &sm.state)
    }
    #[allow(dead_code)]
    pub fn ref_count(&self, idx: usize) -> u32 {
        self.states.get(idx).map(|sm| sm.ref_count).unwrap_or(0)
    }
    #[allow(dead_code)]
    pub fn is_active(&self, idx: usize) -> bool {
        idx < self.states.len() && self.states[idx].ref_count > 0
    }
    pub fn cost_from_start(&self, idx: usize) -> f64 {
        self.states
            .get(idx)
            .map(|sm| sm.cost_from_start)
            .unwrap_or(f64::INFINITY)
    }
    #[allow(dead_code)]
    pub fn set_initial_cost(&mut self, idx: usize, cost: f64) {
        if let Some(sm) = self.states.get_mut(idx) {
            sm.cost_from_start = cost;
        }
    }
    pub fn parent_idx(&self, idx: usize) -> Option<usize> {
        self.states
            .get(idx)
            .and_then(|sm| sm.parent_idx.map(|i| i.get()))
    }
    pub fn component_idx(&self, idx: usize) -> Option<usize> {
        self.states
            .get(idx)
            .and_then(|sm| sm.component_idx.map(|i| i.get()))
    }
    pub fn transition_info(&self, idx: usize) -> Option<&T> {
        self.states
            .get(idx)
            .and_then(|sm| sm.transition_info.as_ref())
    }

    pub fn total_states(&self) -> usize {
        self.states.len()
    }
    #[allow(dead_code)]
    pub fn used_states(&self) -> usize {
        self.states.len() - self.free_indices.len()
    }
    #[allow(dead_code)]
    pub fn free_indices_count(&self) -> usize {
        self.free_indices.len()
    }
    pub fn heap_capacity(&self) -> usize {
        self.states.len()
    }

    pub fn heap_push(&mut self, handle: &StateHandle<S, T>, estimated_total_cost: f64) {
        let idx = handle.index();
        // Guard: grow heap bound before inserting an index beyond current bound
        if idx >= self.heap_bound {
            while self.heap_bound <= idx {
                let _ = grow_heap_if_needed(&mut self.open, idx + 1, &mut self.heap_bound);
            }
            // Rebuild accounting based on current heap content
            self.in_open.clear();
            self.heap_sum_estimated_total_cost = 0.0;
            self.heap_len = 0;
            let mut tmp: Vec<(usize, f64)> = Vec::with_capacity(self.open.len());
            while let Some((i, neg)) = self.open.pop() {
                tmp.push((i, neg));
            }
            for (i, neg) in tmp.into_iter() {
                self.open.push(i, neg);
                self.in_open.insert(i);
                let estimated_total_cost_i = -neg;
                if estimated_total_cost_i.is_finite() {
                    self.heap_sum_estimated_total_cost += estimated_total_cost_i;
                }
                self.heap_len += 1;
            }
        }
        self.increment_ref_count(idx);
        self.open.push(idx, -estimated_total_cost);
        self.in_open.insert(idx);
        if estimated_total_cost.is_finite() {
            self.heap_sum_estimated_total_cost += estimated_total_cost;
        }
        self.heap_len += 1;
    }
    pub fn heap_pop(&mut self) -> Option<StateHandle<S, T>> {
        if let Some((idx, neg_estimated_total_cost)) = self.open.pop() {
            let estimated_total_cost = -neg_estimated_total_cost;
            // Transfer ownership: first create a handle (increments ref_count),
            // then drop the heap's reference (decrement).
            let handle = StateHandle::new(idx, estimated_total_cost, self);
            if estimated_total_cost.is_finite() {
                self.heap_sum_estimated_total_cost -= estimated_total_cost;
            }
            self.heap_len -= 1;
            self.in_open.remove(&idx);
            // Decrement heap membership reference; since the handle was just created,
            // ref_count stays > 0 and metadata (including path_cost) is preserved.
            self.decrement_ref_count(idx);
            Some(handle)
        } else {
            None
        }
    }
    fn heap_decrease_key(&mut self, idx: usize, estimated_total_cost: f64) -> bool {
        if !self.in_open.contains(&idx) {
            return false;
        }
        self.open.decrease_key(&idx, -estimated_total_cost);
        true
    }
    fn is_in_heap(&self, idx: usize) -> bool {
        self.in_open.contains(&idx)
    }
    #[allow(dead_code)]
    pub fn heap_len(&self) -> usize {
        self.heap_len
    }
    pub fn heap_avg_estimated_total_cost(&self) -> f64 {
        if self.heap_len == 0 {
            0.0
        } else {
            self.heap_sum_estimated_total_cost / (self.heap_len as f64)
        }
    }
    pub fn heap_size(&self) -> usize {
        self.open.len()
    }
    #[allow(dead_code)]
    fn heap_mut_for_growth(&mut self) -> &mut QuaternaryHeapOfIndices<usize, f64> {
        &mut self.open
    }
    #[allow(dead_code)]
    fn heap_bound(&self) -> usize {
        self.heap_bound
    }
    #[allow(dead_code)]
    fn set_heap_bound(&mut self, new_bound: usize) {
        self.heap_bound = new_bound;
    }
    #[allow(dead_code)]
    fn heap_bound_mut(&mut self) -> &mut usize {
        &mut self.heap_bound
    }

    pub fn enqueue_or_update_state(
        &mut self,
        state: S,
        path_cost: f64,
        parent: Option<&StateHandle<S, T>>,
        component_idx: usize,
        transition_info: Option<T>,
        estimated_total_cost: f64,
    ) -> bool {
        if path_cost.is_nan()
            || estimated_total_cost.is_nan()
            || path_cost < 0.0
            || estimated_total_cost < 0.0
        {
            return false;
        }
        if let Some(state_idx_nm) = self.try_update_best_cost(state, path_cost) {
            let state_idx = state_idx_nm.get();
            let parent_idx = parent.map(|p| p.index());
            self.set_parent_component_and_transition(
                state_idx,
                parent_idx,
                component_idx,
                transition_info,
            );
            // Ensure heap can accommodate this index BEFORE pushing/decreasing
            if state_idx >= self.heap_bound {
                while self.heap_bound <= state_idx {
                    let _ =
                        grow_heap_if_needed(&mut self.open, state_idx + 1, &mut self.heap_bound);
                }
                // Rebuild accounting structures after growth
                self.in_open.clear();
                self.heap_sum_estimated_total_cost = 0.0;
                self.heap_len = 0;
                let mut tmp: Vec<(usize, f64)> = Vec::with_capacity(self.open.len());
                while let Some((idx, neg_estimated_total_cost)) = self.open.pop() {
                    tmp.push((idx, neg_estimated_total_cost));
                }
                for (idx, neg_estimated_total_cost) in tmp.into_iter() {
                    self.open.push(idx, neg_estimated_total_cost);
                    self.in_open.insert(idx);
                    let estimated_total_cost = -neg_estimated_total_cost;
                    if estimated_total_cost.is_finite() {
                        self.heap_sum_estimated_total_cost += estimated_total_cost;
                    }
                    self.heap_len += 1;
                }
            }
            if self.is_in_heap(state_idx) {
                self.heap_decrease_key(state_idx, estimated_total_cost);
            } else {
                let handle = StateHandle::new(state_idx, estimated_total_cost, self);
                self.heap_push(&handle, estimated_total_cost);
            }
            let heap_capacity = self.heap_capacity();
            grow_heap_if_needed(&mut self.open, heap_capacity, &mut self.heap_bound);
            true
        } else {
            false
        }
    }

    /// Create a handle for a given active index. Increments ref-count for the handle.
    #[allow(dead_code)]
    pub fn make_handle(&mut self, idx: usize, cost: f64) -> StateHandle<S, T> {
        StateHandle::new(idx, cost, self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Hash, PartialEq, Eq, Debug, Default)]
    struct TestState(u32);

    #[derive(Clone, Debug, PartialEq)]
    struct TestTransition {
        action: &'static str,
        cost: f64,
    }

    fn make_pool() -> StatePool<TestState, TestTransition> {
        StatePool::new(1024)
    }

    #[test]
    fn insert_and_lookup_preserves_identity() {
        // Requirement: Pool indexes states and allows retrieving them by index
        let mut pool = make_pool();
        let s = TestState(42);
        let idx = pool.insert_state(s.clone());
        assert_eq!(pool.get_state(idx), Some(&s));
        assert!(!pool.is_active(idx)); // no refs yet
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
        let ok = pool.enqueue_or_update_state(
            TestState(11),
            1.0,
            Some(&parent),
            0,
            Some(TestTransition {
                action: "a",
                cost: 1.0,
            }),
            2.0,
        );
        assert!(ok);
        let bad1 =
            pool.enqueue_or_update_state(TestState(12), f64::NAN, Some(&parent), 0, None, 2.0);
        let bad2 =
            pool.enqueue_or_update_state(TestState(13), 1.0, Some(&parent), 0, None, f64::NAN);
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
