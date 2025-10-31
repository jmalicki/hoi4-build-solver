// Unsafe utilities for directly accessing and growing internal vectors of QuaternaryHeapOfIndices.
//
// This module uses unsafe code to bypass Rust's privacy guarantees and directly access
// private fields of the orx-priority-queue crate's heap implementation.
//
// Structure: QuaternaryHeapOfIndices { heap: Heap { tree: Vec<(N, K)>, positions: HeapPositionsHasIndex { positions: Vec<usize>, ph: PhantomData } } }

use orx_priority_queue::QuaternaryHeapOfIndices;
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
    // Match the exact structure layout from the source code
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

    // Transmute to our internal struct with public fields
    let heap: &mut DaryHeapOfIndicesInternal = unsafe { mem::transmute(open) };

    // Grow the positions vector directly
    heap.heap.positions.positions.resize(new_bound, usize::MAX);
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
) {
    // If we're using more than 90% of the bound, grow by 2x
    if states_len >= (*heap_bound * 9 / 10) {
        let new_bound = *heap_bound * 2;

        // Use unsafe code to directly grow the internal positions vector
        unsafe {
            grow_heap_vector(open, new_bound);
        }

        // heap_prio is stored in states[idx].heap_prio, so it grows automatically with states Vec
        *heap_bound = new_bound;
    }
}
