use std::collections::{HashMap, HashSet};
use std::hash::Hash;
use orx_priority_queue::{PriorityQueue, PriorityQueueDecKey, QuaternaryHeapOfIndices};
use rapidhash::fast::RandomState as RapidHasher;

use crate::heap_growth::grow_heap_if_needed;
use super::{NonMaxUsize, StateHandle};

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
    heap_sum_f: f64,
    heap_len: usize,
}

impl<S: Hash + Eq + Clone + Default, T> StatePool<S, T> {
    pub fn new(initial_heap_bound: usize, start_state: S, start_g: f64) -> (Self, StateHandle<S, T>) {
        let mut pool = Self {
            states: Vec::new(),
            state_to_idx: HashMap::with_hasher(RapidHasher::new()),
            free_indices: Vec::new(),
            open: QuaternaryHeapOfIndices::with_index_bound(initial_heap_bound),
            in_open: HashSet::new(),
            heap_bound: initial_heap_bound,
            heap_sum_f: 0.0,
            heap_len: 0,
        };
        let idx = pool.insert_state(start_state);
        pool.set_initial_cost(idx, start_g);
        let handle = StateHandle::new(idx, start_g, &mut pool);
        (pool, handle)
    }

    fn get_index(&self, state: &S) -> Option<usize> { self.state_to_idx.get(state).copied() }

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

    pub fn insert_state(&mut self, state: S) -> usize {
        let idx = self.allocate_index();
        self.states[idx].state = state.clone();
        self.state_to_idx.insert(state, idx);
        idx
    }

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

    fn set_parent_component_and_transition(&mut self, child_idx: usize, parent_idx: usize, component_idx: usize, transition_info: Option<T>) {
        if let Some(old_parent) = self.states[child_idx].parent_idx { self.decrement_ref_count(old_parent.get()); }
        self.increment_ref_count(parent_idx);
        self.states[child_idx].parent_idx = Some(unsafe { NonMaxUsize::new_unchecked(parent_idx) });
        self.states[child_idx].component_idx = Some(unsafe { NonMaxUsize::new_unchecked(component_idx) });
        self.states[child_idx].transition_info = transition_info;
    }

    pub fn decrement_ref_count(&mut self, idx: usize) {
        if idx >= self.states.len() { return; }
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

    pub fn increment_ref_count(&mut self, idx: usize) { if idx < self.states.len() { self.states[idx].ref_count += 1; } }
    pub fn get_state(&self, idx: usize) -> Option<&S> { self.states.get(idx).map(|sm| &sm.state) }
    pub fn ref_count(&self, idx: usize) -> u32 { self.states.get(idx).map(|sm| sm.ref_count).unwrap_or(0) }
    pub fn is_active(&self, idx: usize) -> bool { idx < self.states.len() && self.states[idx].ref_count > 0 }
    pub fn cost_from_start(&self, idx: usize) -> f64 { self.states.get(idx).map(|sm| sm.cost_from_start).unwrap_or(f64::INFINITY) }
    pub fn set_initial_cost(&mut self, idx: usize, cost: f64) { if let Some(sm) = self.states.get_mut(idx) { sm.cost_from_start = cost; } }
    pub fn parent_idx(&self, idx: usize) -> Option<usize> { self.states.get(idx).and_then(|sm| sm.parent_idx.map(|i| i.get())) }
    pub fn component_idx(&self, idx: usize) -> Option<usize> { self.states.get(idx).and_then(|sm| sm.component_idx.map(|i| i.get())) }
    pub fn transition_info(&self, idx: usize) -> Option<&T> { self.states.get(idx).and_then(|sm| sm.transition_info.as_ref()) }

    pub fn total_states(&self) -> usize { self.states.len() }
    pub fn used_states(&self) -> usize { self.states.len() - self.free_indices.len() }
    pub fn free_indices_count(&self) -> usize { self.free_indices.len() }
    pub fn heap_capacity(&self) -> usize { self.states.len() }

    pub fn heap_push(&mut self, handle: &StateHandle<S, T>, f: f64) {
        let idx = handle.index();
        self.increment_ref_count(idx);
        self.open.push(idx, -f);
        self.in_open.insert(idx);
        if f.is_finite() { self.heap_sum_f += f; }
        self.heap_len += 1;
    }
    pub fn heap_pop(&mut self) -> Option<StateHandle<S, T>> {
        if let Some((idx, neg_f)) = self.open.pop() {
            let f = -neg_f;
            if f.is_finite() { self.heap_sum_f -= f; }
            self.heap_len -= 1;
            self.in_open.remove(&idx);
            self.decrement_ref_count(idx);
            Some(StateHandle::new(idx, f, self))
        } else { None }
    }
    fn heap_decrease_key(&mut self, idx: usize, f: f64) -> bool {
        if !self.in_open.contains(&idx) { return false; }
        self.open.decrease_key(&idx, -f);
        true
    }
    fn is_in_heap(&self, idx: usize) -> bool { self.in_open.contains(&idx) }
    pub fn heap_len(&self) -> usize { self.heap_len }
    pub fn heap_avg_f(&self) -> f64 { if self.heap_len == 0 { 0.0 } else { self.heap_sum_f / (self.heap_len as f64) } }
    pub fn heap_size(&self) -> usize { self.open.len() }
    fn heap_mut_for_growth(&mut self) -> &mut QuaternaryHeapOfIndices<usize, f64> { &mut self.open }
    fn heap_bound(&self) -> usize { self.heap_bound }
    fn set_heap_bound(&mut self, new_bound: usize) { self.heap_bound = new_bound; }
    fn heap_bound_mut(&mut self) -> &mut usize { &mut self.heap_bound }

    pub fn enqueue_or_update_state(&mut self, state: S, cost_value: f64, parent: &StateHandle<S, T>, component_idx: usize, transition_info: Option<T>, f: f64) -> bool {
        if cost_value.is_nan() || f.is_nan() || cost_value < 0.0 || f < 0.0 { return false; }
        if let Some(state_idx_nm) = self.try_update_best_cost(state, cost_value) {
            let state_idx = state_idx_nm.get();
            self.set_parent_component_and_transition(state_idx, parent.index(), component_idx, transition_info);
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


