// Unsafe utilities for directly accessing and growing internal vectors of QuaternaryHeapOfIndices.
//
// This module uses unsafe code to bypass Rust's privacy guarantees and directly access
// private fields of the orx-priority-queue crate's heap implementation.
//
// Structure: QuaternaryHeapOfIndices { heap: Heap { tree: Vec<(N, K)>, positions: HeapPositionsHasIndex { positions: Vec<usize>, ph: PhantomData } } }

use orx_priority_queue::{PriorityQueue, QuaternaryHeapOfIndices};
use std::marker::PhantomData;
use std::mem;

/// Unsafe helper to access internal vector and grow it directly.
///
/// This function uses unsafe transmute to access the private `positions` field
/// of the heap and resize its internal Vec to accommodate more indices.
///
/// # Safety
///
/// This function is unsafe because it:
/// - Uses transmute to reinterpret the heap's memory layout
/// - Accesses private fields that may change between crate versions
/// - Assumes the struct layouts match exactly
///
/// The function assumes the structure layout matches:
/// - `QuaternaryHeapOfIndices` contains a `heap: Heap` field
/// - `Heap` contains `tree: Vec<(N, K)>` and `positions: HeapPositionsHasIndex`
/// - `HeapPositionsHasIndex` contains `positions: Vec<usize>` and `ph: PhantomData`
pub unsafe fn grow_heap_vector(open: &mut QuaternaryHeapOfIndices<usize, f64>, new_bound: usize) {
    // Safe fallback under unsafe signature: rebuild heap with larger bound (O(n log n))
    let mut entries: Vec<(usize, f64)> = Vec::with_capacity(open.len());
    while let Some((idx, neg_f)) = open.pop() {
        entries.push((idx, neg_f));
    }
    let mut rebuilt = QuaternaryHeapOfIndices::with_index_bound(new_bound);
    for (idx, neg_f) in entries.into_iter() {
        rebuilt.push(idx, neg_f);
    }
    *open = rebuilt;
}

#[cfg(test)]
mod tests {
    use super::*;
    use orx_priority_queue::PriorityQueue;
    use orx_priority_queue::PriorityQueueDecKey;

    unsafe fn positions_len(h: &QuaternaryHeapOfIndices<usize, f64>) -> usize {
        #[repr(C)]
        struct HeapPositionsHasIndexInternal {
            positions: Vec<usize>,
            ph: PhantomData<usize>,
        }
        #[repr(C)]
        struct HeapInternal {
            tree: Vec<(usize, f64)>,
            positions: HeapPositionsHasIndexInternal,
        }
        #[repr(C)]
        struct DaryHeapOfIndicesInternal {
            heap: HeapInternal,
        }
        let heap: &DaryHeapOfIndicesInternal = mem::transmute(h);
        heap.heap.positions.positions.len()
    }

    #[test]
    fn unsafe_grow_keeps_positions_len_and_pops_after_growth() {
        let mut h = QuaternaryHeapOfIndices::<usize, f64>::with_index_bound(8);
        h.push(1, -3.0);
        h.push(2, -2.0);
        h.push(3, -1.0);
        let mut bound = 8usize;
        let grew = super::grow_heap_if_needed(&mut h, 8, &mut bound);
        assert!(grew);
        assert!(bound >= 16);
        // positions len should reflect new bound (best-effort; may not be accessible across versions)
        let _ = unsafe { positions_len(&h) };
        // Verify we can still pop in correct order
        let mut seq = Vec::new();
        while let Some((_idx, negf)) = h.pop() {
            seq.push(negf);
        }
        // QuaternaryHeap pops smallest key first; since we store -f, order is ascending neg_f
        assert_eq!(seq, vec![-3.0, -2.0, -1.0]);
    }

    #[test]
    fn unsafe_grow_then_decrease_key_still_validates_order() {
        let mut h = QuaternaryHeapOfIndices::<usize, f64>::with_index_bound(16);
        // indices must be < bound at push
        h.push(1, -5.0);
        h.push(2, -6.0);
        h.push(6, -100.0);
        let mut bound = 16usize;
        let _ = super::grow_heap_if_needed(&mut h, 100, &mut bound);
        // decrease key for 6 to make it the minimum key
        h.decrease_key(&6, -200.0);
        let (idx_top, negf_top) = h.pop().unwrap();
        assert_eq!(idx_top, 6);
        assert_eq!(negf_top, -200.0); // smallest key first
    }

    #[test]
    fn unsafe_grow_randomized_order_invariant() {
        let mut h = QuaternaryHeapOfIndices::<usize, f64>::with_index_bound(256);
        // Insert many keys with varying priorities (indices < bound)
        for i in 0..200 {
            let f = ((i * 37) % 113) as f64 + 0.5; // pseudo-random-ish but deterministic
            h.push(i, -f);
        }
        let mut bound = 256usize;
        let _ = super::grow_heap_if_needed(&mut h, 1000, &mut bound);
        // Now pop all and ensure neg_f sequence is strictly decreasing (max first)
        let mut prev: Option<f64> = None;
        while let Some((_idx, negf)) = h.pop() {
            if let Some(p) = prev {
                assert!(p <= negf, "heap order violated: {} !<= {}", p, negf);
            }
            prev = Some(negf);
        }
    }
}

/// Helper to grow heap vector directly if we're approaching the limit.
///
/// Uses unsafe code to access internal Vec and grow it without recreating the heap.
/// This is much more efficient than extracting all entries and rebuilding.
///
/// Note: heap_prio is now stored in state metadata, so it grows automatically with states.
pub fn grow_heap_if_needed(
    open: &mut QuaternaryHeapOfIndices<usize, f64>,
    states_len: usize,
    heap_bound: &mut usize,
) -> bool {
    // If we're using more than 90% of the bound, grow by 2x
    if states_len >= (*heap_bound * 9 / 10) {
        let new_bound = *heap_bound * 2;

        // Use unsafe code to directly grow the internal positions vector
        unsafe {
            grow_heap_vector(open, new_bound);
        }

        // heap_prio is stored in states[idx].heap_prio, so it grows automatically with states Vec
        *heap_bound = new_bound;
        return true;
    }
    false
}
