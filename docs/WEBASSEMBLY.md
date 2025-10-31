### Running the HOI4 MDP Solver in the Browser via WebAssembly

This document explores making the solver available as a standalone web app (no backend) by compiling the Rust core to WebAssembly (WASM). Users would load a web page, choose options corresponding to the current CLI flags, and compute results entirely in-browser.

## TL;DR

- **Feasible**: Yes, with a small refactor to expose a pure Rust API decoupled from PyO3 and Python types.
- **Approach**: Build a `wasm-bindgen`/`wasm-pack` target that exports a JS-friendly API. Implement a small UI (React/Vite or similar) that parses CSV/Sheets input client-side and calls the WASM solver. Use a Web Worker to keep the UI responsive.
- **Not needed**: Any backend or server-side compute.

## Current Architecture Fit

- Rust core (`rust/hoi4_mdp_core`): Primary compute is in Rust and already largely self-contained. Good candidate for WASM.
- Python layer: Handles CLI, CSV/Sheets I/O, and calls into the Rust library via PyO3. This layer cannot run in-browser. We will replicate only the small amount of I/O/parsing behavior in JS/TS.
- Heuristics: Now pluggable via a trait and factory; selection by string name maps naturally to web UI controls.

## Required Changes

1) Separate Web API from PyO3
- Add a `wasm` feature flag and a sibling interface that exposes:
  - `solve_and_reconstruct_js(nodes: JsValue, target_type: &str, target: i32, opts: SolveOpts) -> JsValue`
  - Types encoded via `serde` + `wasm-bindgen` to/from `JsValue` (JSON-compatible).
- Keep the PyO3 entry point intact for Python, gated behind `cfg(feature = "pyo3")`.

2) JS-friendly data model
- Nodes: `[ [name, numSlots, numInfra, numCivilian, numMilitary], ... ]` (array of tuples) or an object array with named fields. Prefer an object array for readability: `{ name, numSlots, numInfra, numCivilian, numMilitary }`.
- Return: `{ moves: [ { nodeName, action } ], finalState: [ { infra, civ, mil } ], totalCost }`.

3) Logging and progress
- `println!` does not surface in browser console by default. Expose an optional callback via `wasm_bindgen` (e.g., `set_logger(cb)`), or return periodic progress via an async iterator pattern. Simpler: accept a boolean `verbose` and periodically call a JS callback provided by the UI.

4) Long-running compute
- Run the solver in a **Web Worker** (or dedicated Worker in Vite/React) to avoid freezing the main thread.
- Provide a cancel mechanism (e.g., set an atomic flag exposed to Rust via `wasm-bindgen` or cooperative checks).

5) CSV and Google Sheets input
- CSV: Use JS libraries (e.g., Papaparse) to parse CSV in-browser. Transform to the node array for WASM.
- Google Sheets: Use the sheet's CSV export URL directly with `fetch` in the browser. Preprocess columns (including subtracting `Docks`/`Refineries`) on the JS side, to match the Python preprocessor behavior.

## Tooling

- **wasm-bindgen / wasm-pack**: Build Rust to `wasm32-unknown-unknown` and generate a JS wrapper.
- **Bundler**: Vite or Webpack to bundle the generated WASM and the UI.
- **TypeScript**: Define shared types that mirror the Rust `serde` structs.

Typical steps:
```bash
# In crate (rust/hoi4_mdp_core)
wasm-pack build --release --target web --out-dir pkg -- --features wasm

# In web app
npm install ./rust/hoi4_mdp_core/pkg
# or publish pkg to a registry if needed
```

## Compatibility & Limitations

- PyO3: Not available in WebAssembly. The web build must exclude PyO3 bindings.
- Threads: WebAssembly threads require `SharedArrayBuffer` and cross-origin isolation (COOP/COEP) headers. The current solver is single-threaded; OK on normal pages. If we later parallelize, we must enable COOP/COEP and `-Z` atomics features and use a worker with `wasm-bindgen-rayon`.
- Memory: `wasm32` has a practical memory limit (historically ~2–4GB). Large instances could hit memory ceilings. The current solver is optimized for dense indexing and reuse; still, large targets could be memory heavy. Mitigation: show memory usage, add guardrails, allow early cancel.
- Performance: Rust→WASM is fast; however, browser JIT and GC around JS<->WASM boundary introduce some overhead. Avoid frequent cross-boundary calls in hot loops. Batch I/O; pass data in bulk.
- Filesystem: No direct filesystem. Downloads are via `fetch` and user-triggered downloads (e.g., exporting CSV results).
- Timeouts: Browsers can throttle background tabs or long-running tasks. Use Workers and keep UI responsive to avoid the page being considered unresponsive.

## Proposed Web API (Rust)

```rust
#[cfg(feature = "wasm")]
#[wasm_bindgen]
pub struct SolveOpts {
    pub verbose: bool,
    pub print_every: u32,
    pub prune: bool,
    pub heuristic: String, // "best_infra_upper_bound", "djikstra", etc.
}

#[cfg(feature = "wasm")]
#[wasm_bindgen]
pub fn solve_and_reconstruct_js(nodes: JsValue, target_type: String, target: i32, opts: SolveOpts) -> Result<JsValue, JsValue>;
```

Internally, reuse the same search code used by PyO3, with conversions to/from `JsValue` via `serde_wasm_bindgen`.

## UI Sketch

- Inputs:
  - Source: Upload CSV file or paste Google Sheet URL
  - Target type: `military | civilian | factories`
  - Target value: numeric
  - Heuristic: select (default `best_infra_upper_bound`, option `djikstra`)
  - Print cadence / pruning toggle
- Outputs:
  - Progress log (optional)
  - Moves table (download CSV)
  - Final state table (download CSV)
  - Total cost (display and copy button)
- Run control: Start, Cancel

## Benchmarking (Heuristics and Pruning)

The web app can include a benchmarking mode to compare search performance across heuristics and pruning settings, fully in-browser:

- Configuration:
  - Select multiple heuristics to test (checkboxes)
  - Toggle pruning on/off per heuristic (2× matrix)
  - Fixed inputs (same nodes/target) across runs
  - Optional random seeds / repeats (for future stochastic features; currently deterministic)

- Collected metrics per run:
  - `totalCost` (optimal path cost found)
  - `wallTimeMs`
  - `expandedStates` (iters)
  - `generatedStates` (total enqueues/updates)
  - `prunedCount` (if pruning enabled)
  - `heapPeak` and `heapAvgF`
  - `totalStates` (unique states in pool)

- Presentation:
  - Table comparing all runs (rows = heuristic×pruning)
  - Bar charts for `wallTimeMs`, `expandedStates`, `prunedCount`
  - Download CSV of raw metrics

- API considerations:
  - Expose a `solve_with_metrics_js(...) -> JsValue` variant returning both result and metrics
  - Metrics gathered from the existing debug counters in the Rust core
  - Ensure minimal cross-boundary calls; return metrics in one object at the end of each run

- Execution model:
  - Each benchmark run executes sequentially in a Web Worker to avoid contention
  - Progress per run streamed via callback (optional)

## Incremental Plan

1) Add `wasm` feature and JS API wrappers (behind `cfg`), keep PyO3 intact.
2) Build with `wasm-pack`, wire up a minimal HTML+JS demo that hardcodes small inputs.
3) Create a simple Vite/React app; add CSV upload and Google Sheets fetch.
4) Move solver to a Web Worker; add progress callback and cancel.
5) Polish UI; pretty-print counts, durations, and pruning stats like the CLI.

## Risks & Mitigations

- "A* exhausted" or correctness issues: The same core code runs. Unit tests can be shared. Add a small regression test dataset in the web demo.
- Large inputs stall UI: Use a Worker, stream progress, and provide Cancel.
- WASM packaging complexity: Keep the surface API small; prefer `serde` JSON-like objects.

## Conclusion

Converting the solver to a browser-based, no-backend web app is practical. The main work is exposing a WASM-friendly API and building a small UI. We retain the Python CLI for local workflows and add a WebAssembly target for interactive, shareable demos and distribution.


