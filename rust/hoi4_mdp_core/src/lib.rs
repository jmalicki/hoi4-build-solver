// High-performance A* solver for the HOI4 build planning MDP.
//
// This crate implements the core search loop and domain logic in Rust and exposes
// a minimal Python API via PyO3. The Python layer handles CSV/Sheets I/O,
// while Rust manages: state representation, successor generation, heuristic,
// and the A* frontier with a binary heap.
//
// Key modeling choices follow docs/MODELING.md:
// - Deterministic transitions; one node changes per action.
// - Immediate cost per action = base_cost / infra_multiplier / sumCivilian,
//   with the denominator clamped at 1 to avoid division by zero.
// - Heuristic is an admissible lower bound using best-case infra and an upper
//   bound on future civilians: civUpper = civ + max(0, empty - remainingMil).
//
use pyo3::prelude::*;
use pyo3::types::PyAny;
use pyo3::exceptions::PyException;
use smallvec::SmallVec;
use std::cmp::Ordering;
use std::io::{self, Write};

mod heap_growth;
mod heuristic;
mod core;
mod state_pool;
use state_pool::{StatePool, StateHandle};
use heuristic::{Heuristic, create_by_name};
// Custom Python exception to signal user-requested early stop
pyo3::create_exception!(hoi4_mdp_core, SearchStoppedError, PyException);

/// Static descriptor of a node (immutable across search).
#[derive(Clone, Copy)]
pub(crate) struct NodeDesc {
    slots: u8, // 0..=255
}

/// Per-node dynamic state tracked during search.
///
/// - infra: infrastructure level in [0,5]
/// - civ: number of civilian factories (>= 0)
/// - mil: number of military factories (>= 0)
#[derive(Clone, Copy, Eq, PartialEq, Hash)]
struct NodeState {
    infra: u8, // 0..=5
    civ: u8,   // 0..=255 (see docs)
    mil: u8,   // 0..=255 (see docs)
}

/// Joint state across all nodes.
///
/// Stored as a vector to support variable numbers of nodes; hashing and equality
/// are defined via derives on the inner elements and vector content.
#[derive(Clone, Eq, PartialEq, Hash, Default)]
pub(crate) struct State(pub(crate) Vec<NodeState>);

/// Target type for the MDP goal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TargetType {
    Military,
    Civilian,
    Factories, // Total factories (military + civilian)
}

/// Return true if a joint state meets the terminal goal.
///
/// The terminal condition depends on the target type:
/// - Military: total military factories >= target
/// - Civilian: total civilian factories >= target
/// - Factories: total factories (military + civilian) >= target
pub(crate) fn is_terminal(st: &State, target_type: TargetType, target: i32) -> bool {
    match target_type {
        TargetType::Military => {
            let mut sum = 0i32;
            for ns in &st.0 {
                sum += ns.mil as i32;
            }
            sum >= target
        }
        TargetType::Civilian => {
            let mut sum = 0i32;
            for ns in &st.0 {
                sum += ns.civ as i32;
            }
            sum >= target
        }
        TargetType::Factories => {
            let mut sum = 0i32;
            for ns in &st.0 {
                sum += ns.mil as i32 + ns.civ as i32;
            }
            sum >= target
        }
    }
}


fn fmt_count(n: usize) -> String {
    if n >= 1_000_000_000 {
        format!("{:.2}B", (n as f64) / 1_000_000_000.0)
    } else if n >= 1_000_000 {
        format!("{:.2}M", (n as f64) / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.2}K", (n as f64) / 1_000.0)
    } else {
        n.to_string()
    }
}

fn fmt_step(n: usize) -> String {
    if n.is_multiple_of(1_000_000_000) && n >= 1_000_000_000 {
        format!("{}B", n / 1_000_000_000)
    } else if n.is_multiple_of(1_000_000) && n >= 1_000_000 {
        format!("{}M", n / 1_000_000)
    } else if n.is_multiple_of(1_000) && n >= 1_000 {
        format!("{}K", n / 1_000)
    } else {
        n.to_string()
    }
}
/// Snapshot of solver progress, exposed to Python as a read-only class.
#[pyclass]
#[derive(Clone)]
struct PyProgressSnapshot {
    #[pyo3(get)]
    iterations: usize,
    #[pyo3(get)]
    cost_from_start: f64,
    #[pyo3(get)]
    heap_size: usize,
    #[pyo3(get)]
    total_states: usize,
    #[pyo3(get)]
    avg_f: f64,
    #[pyo3(get)]
    pruned: usize,
    #[pyo3(get)]
    best_upper_bound: f64,
}



/// Information about a successor state generated from an action.
#[derive(Clone)]
struct Successor {
    /// Index of the node that was acted upon
    node_index: usize,
    /// Action label ("civilian", "military", "infra", "convert")
    action: &'static str,
    /// The resulting state after applying the action
    next_state: State,
    /// Step cost for this action (using pre-action total civilian count as denominator)
    step_cost: f64,
}

/// Domain-specific transition information for path reconstruction.
///
/// This is stored in StatePool alongside generic information (parent_idx, component_idx).
/// The pool handles all storage and retrieval - lib.rs doesn't need to manage it.
pub(crate) struct TransitionInfo {
    /// Action label ("civilian", "military", "infra", "convert") - domain-specific
    action: &'static str,
    /// Step cost from parent to this state - domain-specific (could be generic, but stored here for convenience)
    cost: f64,
}

/// Generate feasible successors for a state.
///
/// Yields `Successor` structs containing node_index, action, next_state, and step_cost.
/// The per-step cost uses the pre-action total civilian count as denominator.
pub(crate) fn iter_successors<'a>(
    st: &'a State,
    nodes: &'a [NodeDesc],
) -> impl Iterator<Item = Successor> + 'a {
    let n_nodes = st.0.len();
    let civ_den = st.0.iter().map(|ns| ns.civ as i32).sum::<i32>().max(1) as f64;
    st.0.iter().enumerate().flat_map(move |(i, ns)| {
        let nd = &nodes[i];
        let mult = infra_mult(ns.infra);
        let mut out: SmallVec<[Successor; 4]> = SmallVec::new();
        // civilian
        if (ns.civ + ns.mil) < nd.slots {
            let mut v = Vec::with_capacity(n_nodes);
            v.extend_from_slice(&st.0);
            v[i].civ += 1;
            out.push(Successor {
                node_index: i,
                action: "civilian",
                next_state: State(v),
                step_cost: 10800.0 / mult / civ_den,
            });
        }
        // military
        if (ns.civ + ns.mil) < nd.slots {
            let mut v = Vec::with_capacity(n_nodes);
            v.extend_from_slice(&st.0);
            v[i].mil += 1;
            out.push(Successor {
                node_index: i,
                action: "military",
                next_state: State(v),
                step_cost: 7200.0 / mult / civ_den,
            });
        }
        // infra
        if ns.infra < 5 {
            let mut v = Vec::with_capacity(n_nodes);
            v.extend_from_slice(&st.0);
            v[i].infra += 1;
            out.push(Successor {
                node_index: i,
                action: "infra",
                next_state: State(v),
                step_cost: 6000.0 / mult / civ_den,
            });
        }
        // convert: civ -> mil keeps (civ + mil) unchanged; capacity already ensured elsewhere
        if ns.civ >= 1 {
            let mut v = Vec::with_capacity(n_nodes);
            v.extend_from_slice(&st.0);
            v[i].civ -= 1;
            v[i].mil += 1;
            out.push(Successor {
                node_index: i,
                action: "convert",
                next_state: State(v),
                step_cost: 4000.0 / mult / civ_den,
            });
        }
        out
    })
}

/// Solve the problem and reconstruct the plan in one call (Python API).
///
/// Parameters (Python side):
/// - nodes: list of tuples (name, numSlots, numInfra, numCivilian, numMilitary)
/// - target_type: "military", "civilian", or "factories"
/// - target: desired target value for the selected type
/// - verbose: print progress lines
/// - print_every: cadence in expansions for progress lines
///
/// Returns (moves, final_state, total_cost):
/// - moves: list of (nodeName, actionLabel)
/// - final_state: list of per-node triples (infra, civ, mil)
/// - total_cost: cost along the optimal plan
/// solve_and_reconstruct(nodes, target_type, target, *, verbose=True, print_every=1)
///
/// Python entry point. Types:
/// - nodes: list[tuple[str, int, int, int, int]]
/// - target_type: str ("military", "civilian", or "factories")
/// - target: int
/// - print_every: int (kw-only, cadence for progress callback; 0 disables)
/// - prune: bool (kw-only)
/// - heuristic: str (kw-only, heuristic name)
/// - progress_callback: Optional[Callable[[ProgressSnapshot], None]] (kw-only)
/// Returns tuple[list[(str,str)], list[(int,int,int)], float]
#[pyfunction]
#[pyo3(
    signature = (nodes, target_type, target, *, print_every=1, prune=false, heuristic="best_infra_upper_bound", progress_callback=None),
    text_signature = "(nodes: list[tuple[str,int,int,int,int]], target_type: str, target: int, *, print_every: int = 1, prune: bool = False, heuristic: str = 'best_infra_upper_bound', progress_callback: Optional[Callable[[ProgressSnapshot], bool]] = None) -> tuple[list[tuple[str,str]], list[tuple[int,int,int]], float]"
)]
fn solve_and_reconstruct(
    _py: Python<'_>,
    nodes: Vec<(String, i32, i32, i32, i32)>,
    target_type: String,
    target: i32,
    print_every: usize,
    prune: bool,
    heuristic: String,
    progress_callback: Option<Bound<PyAny>>,
) -> PyResult<(Vec<(String, String)>, Vec<(i32, i32, i32)>, f64)> {
    // Parse target type
    let target_type_enum = match target_type.to_lowercase().as_str() {
        "military" => TargetType::Military,
        "civilian" => TargetType::Civilian,
        "factories" => TargetType::Factories,
        _ => {
            return Err(pyo3::exceptions::PyValueError::new_err(format!(
                "Invalid target_type: {}. Must be 'military', 'civilian', or 'factories'",
                target_type
            )));
        }
    };
    fn to_u8(_py: Python<'_>, v: i32, field: &str, max: u8) -> PyResult<u8> {
        if v < 0 || v as u32 > max as u32 {
            Err(pyo3::exceptions::PyValueError::new_err(format!(
                "{} out of range (0..={})",
                field, max
            )))
        } else {
            Ok(v as u8)
        }
    }
    let names: Vec<String> = nodes.iter().map(|t| t.0.clone()).collect();
    let mut desc: Vec<NodeDesc> = Vec::with_capacity(nodes.len());
    for t in &nodes {
        let slots = to_u8(_py, t.1, "numSlots", 255)?;
        desc.push(NodeDesc { slots });
    }
    let n_nodes = nodes.len();
    let mut st_vec: Vec<NodeState> = Vec::with_capacity(n_nodes);
    for t in &nodes {
        let infra = to_u8(_py, t.2, "numInfra", 5)?;
        let civ = to_u8(_py, t.3, "numCivilian", 255)?;
        let mil = to_u8(_py, t.4, "numMilitary", 255)?;
        st_vec.push(NodeState { infra, civ, mil });
    }
    // st_vec is already exactly sized (capacity = len) since we used with_capacity and pushed exactly n_nodes
    let st = State(st_vec);

    // Feasibility quick check
    let total_slots: i32 = desc.iter().map(|d| d.slots as i32).sum();
    if target > total_slots {
        return Err(pyo3::exceptions::PyValueError::new_err(
            "target exceeds capacity",
        ));
    }

    // Build core options and callback
    let mut core_cb = progress_callback.map(|cb| {
        move |snap: &core::ProgressSnapshot| -> bool {
            let py_snap = PyProgressSnapshot {
                iterations: snap.iterations,
                cost_from_start: snap.cost_from_start,
                heap_size: snap.heap_size,
                total_states: snap.total_states,
                avg_f: snap.avg_f,
                pruned: snap.pruned,
                best_upper_bound: snap.best_upper_bound,
            };
            Python::with_gil(|py| {
                match cb.call1((py_snap,)) {
                    Ok(val) => val.extract::<bool>().unwrap_or(false),
                    Err(_) => false,
                }
            })
        }
    });
    let opts = core::SolveOptions {
        prune,
        print_every,
        heuristic_name: &heuristic,
        progress_cb: core_cb.as_mut(),
    };
    let (moves_idx, final_state_rs, total_cost) = core::solve_and_reconstruct_core(desc, st.clone(), target_type_enum, target, opts);
    let final_state: Vec<(i32, i32, i32)> = final_state_rs.0.iter().map(|ns| (ns.infra as i32, ns.civ as i32, ns.mil as i32)).collect();
    let moves: Vec<(String, String)> = moves_idx.into_iter().map(|(i, action)| (names[i].clone(), action.to_string())).collect();
    Ok((moves, final_state, total_cost))
}

/// Python module initializer for `hoi4_mdp_core`.
#[pymodule]
fn hoi4_mdp_core(_py: Python, m: &Bound<PyModule>) -> PyResult<()> {
    // Module docstring
    m.setattr(
        "__doc__",
        "Rust A* core for HOI4 MDP: solve_and_reconstruct(nodes, target_military, *, verbose=True, print_every=1)",
    )?;
    m.add_function(wrap_pyfunction!(solve_and_reconstruct, m)?)?;
    // Export the custom exception type
    m.add("SearchStoppedError", _py.get_type::<SearchStoppedError>())?;
    Ok(())
}
// TEST REBUILD 1761866152
