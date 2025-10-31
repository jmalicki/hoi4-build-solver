use std::hash::Hash;
use super::StatePool;

pub struct StateHandle<S: Hash + Eq + Clone + Default, T> {
    pub(crate) idx: usize,
    pub(crate) cost: f64,
    pub(crate) pool_ptr: *mut StatePool<S, T>,
}

unsafe impl<S: Hash + Eq + Clone + Default, T> Send for StateHandle<S, T> {}
unsafe impl<S: Hash + Eq + Clone + Default, T> Sync for StateHandle<S, T> {}

impl<S: Hash + Eq + Clone + Default, T> StateHandle<S, T> {
    pub fn cost(&self) -> f64 { self.cost }
    pub fn cost_from_start(&self, pool: &StatePool<S, T>) -> f64 { pool.cost_from_start(self.idx) }
    pub fn state<'a>(&self, pool: &'a StatePool<S, T>) -> Option<&'a S> { pool.get_state(self.idx) }
    pub fn parent(&self, pool: &mut StatePool<S, T>) -> Option<StateHandle<S, T>> {
        pool.parent_idx(self.idx).map(|parent_idx| {
            let parent_f = pool.get_state(parent_idx).map(|_| pool.cost_from_start(parent_idx)).unwrap_or(0.0);
            StateHandle::new(parent_idx, parent_f, pool)
        })
    }
    pub fn transition_info<'a>(&self, pool: &'a StatePool<S, T>) -> Option<&'a T> { pool.transition_info(self.idx) }
    pub fn component_index(&self, pool: &StatePool<S, T>) -> Option<usize> { pool.component_idx(self.idx) }
    pub(crate) fn index(&self) -> usize { self.idx }
    pub(crate) fn new(idx: usize, cost: f64, pool: &mut StatePool<S, T>) -> Self {
        pool.increment_ref_count(idx);
        StateHandle { idx, cost, pool_ptr: pool }
    }
}

impl<S: Hash + Eq + Clone + Default, T> Drop for StateHandle<S, T> {
    fn drop(&mut self) { unsafe { (*self.pool_ptr).decrement_ref_count(self.idx); } }
}


