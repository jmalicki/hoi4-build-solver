# Progress and Metrics Callback Design

Goal: Replace internal string printing with a callback-based progress reporting API usable by both Python (CLI) and WebAssembly (Web UI). The core remains free of UI/IO concerns.

## Summary

- Introduce a lightweight, read-only `ProgressSnapshot` struct in Rust capturing the same variables we currently print.
- Core solver accepts an optional progress callback. When provided, it is invoked at a configurable cadence (replacing `verbose`/`print_every`).
- Front-ends (Python CLI, Web UI) decide how to display/log (strings, tables, charts) based on the snapshot.
- No string formatting in the core; only data.

## Data Model

```rust
pub struct ProgressSnapshot {
    pub iterations: usize,        // expanded count
    pub cost_from_start: f64,     // current state's g
    pub heap_size: usize,
    pub total_states: usize,
    pub avg_f: f64,               // pool.heap_avg_f()
    pub pruned: usize,            // if pruning enabled
    pub best_upper_bound: f64,    // anytime UB, if tracked
}
```

- Read-only fields; no accessors necessary.
- Kept small and stable for cross-language bindings.

## Core API Changes

1) Replace `verbose: bool` and `print_every: usize` with:

```rust
pub struct ProgressOptions {
    pub cadence: usize, // 0 disables
}

pub type ProgressCallback = dyn Fn(&ProgressSnapshot) -> bool + Send + Sync + 'static; // return true to request early stop

pub struct SolveOptions {
    pub prune: bool,
    pub heuristic: String,
    pub progress: Option<(ProgressOptions, Box<ProgressCallback>)>,
}
```

2) Core entry points accept `SolveOptions` and invoke the callback when:

- `iterations == 1` or `iterations % cadence == 0`
- Final summary (last snapshot) before returning

3) The callback is not called from within tight inner loops repeatedly; only at cadence boundaries to minimize overhead.

## Python Front-end (PyO3)

- Expose `ProgressSnapshot` as a simple class with read-only attributes via PyO3 (dataclass-like behavior). No methods.
- Accept a Python callable for progress, e.g.:

```python
def solve_and_reconstruct(..., *, prune: bool, heuristic: str, progress=None, print_every: int = 10_000):
    # if progress is not None: wrap into SolveOptions.progress; return True from callback to stop
```

- If `progress` is provided, disable internal printing; user decides verbosity.
- Backward compatible: if `progress is None`, keep existing banner/final line printing for now (deprecated path), or disable printing entirely and only use the callback path.

## WebAssembly Front-end

- Expose `ProgressSnapshot` via `wasm-bindgen` as a JS-friendly object (serde-based or explicit getters).
- Accept a JS function (or closure) as the callback returning boolean; for the Web UI, proxy to a Web Worker `postMessage` for UI updates. Return `true` to request early stop.

## Metrics (End-of-run)

- In addition to progress, return a final metrics object alongside the solution:

```rust
pub struct SolveMetrics {
    pub iterations: usize,
    pub generated: usize,
    pub pruned: usize,
    pub heap_peak: usize,
    pub total_states: usize,
    pub wall_time_ms: u128,
    pub best_upper_bound: f64,
}
```

- This can be returned in both Python and WASM APIs (e.g., `solve_with_metrics_*`).

## Threading & Safety

- The callback must be `Send + Sync` and lightweight. Avoid heavy work in the callback; UI should batch/queue updates.
- For WASM, callbacks run on the Worker thread; message to main thread via `postMessage`.

## Migration Plan

1) Define `ProgressSnapshot`, `ProgressOptions`, `SolveOptions`, and add callback support in the core.
2) Replace direct println! calls with snapshot creation and callback invocation.
3) Update Python API to accept `progress` and `print_every`; stop printing internally when a callback is present.
4) Update Web doc to use the callback for progress bars/logs.
5) Add optional `solve_with_metrics_*` variants to return final metrics.

### Early Stop Semantics

When the callback returns `true`, the solver halts and returns the best-so-far path based on minimal `f = g + h` encountered (not strictly the minimal `g`). This provides a reasonable anytime result with the lowest estimated total cost seen so far.

Why not return minimal `g` (cost-so-far)?

- Different depths: A node with very low `g` may simply be shallow (few steps taken) while a competing node with higher `g` is much closer to the goal. Comparing raw `g` across unequal depths is misleading.
- Remaining work ignored: `g` ignores the cost to complete the plan. Two frontiers with similar `g` can have very different remaining cost. `f = g + h` incorporates a principled lower bound on the remainder.
- Our dynamics amplify this: denominators and multipliers change as the plan progresses (civilians, infra), so early cheap actions can make `g` look artificially small relative to deeper, more promising states.

Conclusion: `f` is the correct anytime signal; it balances progress so far with a consistent lower bound on what remains, avoiding the bias that makes `g` incomparable mid-search.

## Rationale

- Clean separation of compute (core) from presentation (front-ends).
- Reusable across Python CLI, Node CLI, and Web UI.
- Enables rich UI: progress bars, charts, benchmarking dashboards, without changing core code.
