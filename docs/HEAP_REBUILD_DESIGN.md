### Heap Rebuild Design (O(n) vs O(n log n))

Problem

- We maintain the A* frontier with `orx-priority-queue::QuaternaryHeapOfIndices<usize, f64>`.
- When the number of states approaches the heap's index bound, we need to increase the bound to avoid index out-of-bounds.
- Our initial fix popped all entries and reinserted into a larger heap (O(n log n)). While correct and safe, it is suboptimal compared to an O(n) heapify rebuild.

Goal

- Rebuild the heap in O(n) time when the index bound needs to grow.
- Avoid unsafe introspection of private fields in third-party crates.
- Preserve invariants: in-heap membership (`in_open`), average f tracking, and reference counts.

Current Data Structures

- `open: QuaternaryHeapOfIndices<usize, f64>` stores `(state_idx, -f)` pairs.
- `in_open: HashSet<usize>` tracks indices currently in the heap.
- `heap_sum_f: f64` and `heap_len: usize` track average f for progress reporting.

Constraints

- `orx-priority-queue` does not expose a public O(n) heap-building API from an existing collection in the current usage pattern.
- We want to avoid unsafe memory transmutation of private internals (see removed `heap_growth.rs`).

Design Options

1) Mirror open entries for O(n) heapify if the crate supports bulk build
   - Maintain a shadow `Vec<(usize, f64)>` of heap entries (`neg_f`) updated on every push/decrease/pop.
   - On rebuild, if the crate exposes a constructor like `from_vec_with_bound(entries, new_bound)` that performs heapify O(n), use it to replace `open` and refresh `in_open`, `heap_sum_f`, `heap_len` from the mirrored vector.
   - Complexity: O(n) if bulk build API exists; otherwise falls back to O(n log n) via pushes.

2) Fallback safe rebuild (current implementation)
   - Drain the heap with successive `pop()` calls (O(n log n)) and push into a new heap of larger bound.
   - Recompute `in_open`, `heap_sum_f`, and `heap_len` during reinsertion.
   - Simple, safe, and does not rely on internal APIs; acceptable for initial correctness.

3) Request or implement a bulk-build API upstream
   - Contribute a PR to `orx-priority-queue` to add a public constructor that heapifies from entries with a given index bound.
   - Then switch our rebuild path to O(n) once available.

Chosen Approach (Phased)

- Phase 1 (done): Implement safe rebuild by draining and reinserting (O(n log n)). Remove unsafe memory hacks.
- Phase 2 (near-term): Add a mirrored `open_entries: Vec<(usize, f64)>` inside `StatePool` to keep exact heap content updated on push/decrease/pop. This positions us to switch to O(n) rebuild when a bulk-build API is available.
- Phase 3 (upstream/API): If/when `orx-priority-queue` exposes a bulk-build or heapify method, replace the reinsertion loop with a single O(n) build using `open_entries`.

Correctness & Invariants

- During rebuild:
  - We reconstruct `in_open` by inserting every `(idx, neg_f)` entry.
  - `heap_sum_f` and `heap_len` are recomputed consistently (`f = -neg_f`).
  - Reference counting is unaffected because heap membership increments were already accounted for when entries were first pushed; rebuild does not alter handles or state metadata.

Complexity

- Current: O(n log n) per rebuild (n = heap size). Rebuild triggers when `states_len >= 0.9 * heap_bound`, then doubles bound; amortized frequency is low.
- Target: O(n) per rebuild once bulk-build is possible.

Tests

- Added `heap_rebuilds_when_near_capacity` unit test to ensure no panic and that heap grows with tiny initial bounds.
- Future: add property-style tests to validate that order (by f) is preserved after rebuild.

Notes

- We deliberately removed the previous unsafe approach that mutated internal heap vectors. It was fragile against upstream changes and caused index out-of-bounds panics.
- The phased plan keeps the system correct and debuggable now, while laying groundwork for an O(n) optimization without unsafe code.

Unsafe Growth (Pinned Version) — Why It’s Okay

- We pinned `orx-priority-queue` to `=1.7.0` and implemented an unsafe growth that resizes the internal positions vector in place. This avoids the O(n log n) rebuild and keeps heap content intact without re-pushes.
- Why acceptable now:
  - The exact internal layout for 1.7.0 was verified and encoded with `#[repr(C)]` mirror structs in `rust/hoi4_mdp_core/src/heap_growth.rs`.
  - We added invariant-heavy tests that run after growth:
    - Pop order remains correct; `decrease_key` works post-growth.
    - Internal positions vector length reflects the new bound.
    - Randomized workload preserves heap ordering after growth.
  - The growth only extends capacity; it does not mutate existing keys, nodes, or positions for live indices, preserving heap semantics.
- Risk controls:
  - Version pin prevents silent upstream layout changes.
  - Tests will catch regressions if the layout changes or semantics drift.
  - If we change crate version, we must update/revalidate the mirror layout and rerun tests.

Why better than other proposals (for now)

- Versus O(n log n) rebuild: avoids repeated `push` cost and churn, preserves exact heap content and priorities, and eliminates transient state where `in_open` must be recomputed from scratch.
- Versus forking the crate: no additional maintenance surface or divergence; safer operationally with pinning and tests, and easy to roll back.
- Versus writing our own heap: faster path to performance without reimplementing mature behavior; we keep focus on domain logic.
- Versus waiting for upstream heapify API: delivers performance immediately while keeping a migration path if/when bulk-build is added.

Trade-offs and Migration Plan

- We deliberately removed the previous broad unsafe hacks and replaced them with a narrow, capacity-only resize guarded by tests. If upstream adds safe APIs, we will migrate and delete the unsafe code. If the crate updates or we unpin, CI must fail until we re-validate the mirror layout and tests.
